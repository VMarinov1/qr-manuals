use std::path::Path as FsPath;

use axum::{
    body::Body,
    extract::{
        multipart::{Field, MultipartError},
        Multipart, Path, Request, State,
    },
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Form,
};
use axum_extra::extract::cookie::CookieJar;
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use serde::Deserialize;
use tokio::io::AsyncWriteExt;
use tower::ServiceExt;
use tower_http::services::ServeFile;

use super::{
    auth::CurrentUser,
    error::{AppError, AppResult},
    flash::Flash,
    AppState,
};
use crate::{
    manuals::{self, NewFile},
    products,
    storage::{self, FileKind, TEMP_PREFIX},
};

pub const MAX_FILES_PER_UPLOAD: usize = 20;

fn manual_url(product_id: i64, language: &str) -> String {
    format!("/products/{product_id}?lang={language}#manuals")
}

pub async fn upload(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path((product_id, language)): Path<(i64, String)>,
    jar: CookieJar,
    mut multipart: Multipart,
) -> AppResult<Response> {
    products::get(&state.db, product_id).await?.ok_or(AppError::NotFound)?;
    let languages = products::languages(&state.db).await?;
    if !languages.iter().any(|l| l.code == language) {
        return Err(AppError::NotFound);
    }

    let dir = &state.config.upload_dir;
    let max = state.config.max_upload_bytes;
    let mut saved = 0usize;
    let mut rejected = Vec::new();
    let mut received = 0usize;

    loop {
        let mut field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(err) => {
                rejected.push(interrupted(&err));
                break;
            }
        };
        if field.name() != Some("files") {
            continue;
        }
        let original = storage::clean_original_name(field.file_name().unwrap_or_default());
        if original.is_empty() {
            continue; // полето е изпратено без избран файл
        }
        received += 1;
        if received > MAX_FILES_PER_UPLOAD {
            rejected.push(format!("{original}: най-много {MAX_FILES_PER_UPLOAD} файла наведнъж"));
            continue;
        }

        match receive_file(dir, &mut field, max).await {
            Ok(Received::Saved { stored_name, kind, size }) => {
                let file = NewFile {
                    original_name: &original,
                    stored_name: &stored_name,
                    kind,
                    size_bytes: size as i64,
                };
                if let Err(err) = manuals::add_file(&state.db, product_id, &language, &file).await {
                    storage::remove_files(dir, &[stored_name]).await;
                    return Err(err.into());
                }
                saved += 1;
            }
            Ok(Received::Rejected(reason)) => rejected.push(format!("{original}: {reason}")),
            Err(ReceiveError::Multipart(err)) => {
                rejected.push(interrupted(&err));
                break;
            }
            Err(ReceiveError::Io(err)) => return Err(err.into()),
        }
    }

    tracing::info!(product_id, %language, saved, rejected = rejected.len(), by = %user.email, "качване");

    let mut lines = Vec::new();
    if saved > 0 {
        lines.push(format!("Качени файлове: {saved}"));
    } else if rejected.is_empty() {
        lines.push("Не е избран файл.".to_owned());
    }
    let error = saved == 0 || !rejected.is_empty();
    lines.extend(rejected);

    let jar = Flash { error, lines }.set(jar);
    Ok((jar, Redirect::to(&manual_url(product_id, &language))).into_response())
}

fn interrupted(err: &MultipartError) -> String {
    if err.status() == StatusCode::PAYLOAD_TOO_LARGE {
        "Общият размер е твърде голям. Качи файловете на няколко пъти.".to_owned()
    } else {
        tracing::warn!("прекъснато качване: {err}");
        "Качването беше прекъснато.".to_owned()
    }
}

enum Received {
    Saved {
        stored_name: String,
        kind: FileKind,
        size: usize,
    },
    Rejected(String),
}

enum ReceiveError {
    Multipart(MultipartError),
    Io(std::io::Error),
}

/// Записва файла във временен файл, проверява размера и вида и го премества
/// под окончателното име. Временният файл се изтрива при всеки отказ.
async fn receive_file(dir: &FsPath, field: &mut Field<'_>, max: usize) -> Result<Received, ReceiveError> {
    let temp = dir.join(format!("{TEMP_PREFIX}{}", storage::random_hex(8)));
    let outcome = match write_temp(&temp, field, max).await {
        Ok(Some((_, 0))) => Ok(Received::Rejected("файлът е празен".into())),
        Ok(Some((head, size))) => match FileKind::detect(&head) {
            Some(kind) => {
                let stored_name = storage::new_stored_name(kind);
                match tokio::fs::rename(&temp, storage::path_for(dir, &stored_name)).await {
                    Ok(()) => return Ok(Received::Saved { stored_name, kind, size }),
                    Err(err) => Err(ReceiveError::Io(err)),
                }
            }
            None => Ok(Received::Rejected("не е снимка (JPG, PNG, WEBP) или PDF".into())),
        },
        Ok(None) => Ok(Received::Rejected(format!(
            "по-голям от {} MB",
            max / 1024 / 1024
        ))),
        Err(err) => Err(err),
    };
    tokio::fs::remove_file(&temp).await.ok();
    outcome
}

/// Връща първите байтове и размера, или None, ако файлът надвиши лимита.
async fn write_temp(
    path: &FsPath,
    field: &mut Field<'_>,
    max: usize,
) -> Result<Option<(Vec<u8>, usize)>, ReceiveError> {
    let mut out = tokio::fs::File::create(path).await.map_err(ReceiveError::Io)?;
    let mut head = Vec::with_capacity(FileKind::HEAD_LEN);
    let mut size = 0usize;

    while let Some(chunk) = field.chunk().await.map_err(ReceiveError::Multipart)? {
        size += chunk.len();
        if size > max {
            return Ok(None);
        }
        if head.len() < FileKind::HEAD_LEN {
            let take = (FileKind::HEAD_LEN - head.len()).min(chunk.len());
            head.extend_from_slice(&chunk[..take]);
        }
        out.write_all(&chunk).await.map_err(ReceiveError::Io)?;
    }
    out.flush().await.map_err(ReceiveError::Io)?;
    Ok(Some((head, size)))
}

/// Публично сервиране на качен файл. Имената са случайни и не се променят,
/// затова браузърът може да ги кешира без проверка.
pub async fn serve(
    State(state): State<AppState>,
    Path(stored_name): Path<String>,
    request: Request,
) -> AppResult<Response> {
    if !storage::is_valid_stored_name(&stored_name) {
        return Err(AppError::NotFound);
    }
    let file = manuals::find_by_stored_name(&state.db, &stored_name)
        .await?
        .ok_or(AppError::NotFound)?;

    let path = storage::path_for(&state.config.upload_dir, &file.stored_name);
    // ServeFile поддържа Range заявки, които телефоните ползват за големи PDF файлове.
    let mut response = ServeFile::new(path)
        .oneshot(request)
        .await
        .map(|res| res.map(Body::new))
        .unwrap_or_else(|never| match never {});

    if response.status() == StatusCode::NOT_FOUND {
        tracing::error!(file = %file.stored_name, "файлът липсва на диска");
        return Err(AppError::NotFound);
    }

    let headers = response.headers_mut();
    if let Ok(mime) = HeaderValue::from_str(&file.mime) {
        headers.insert(header::CONTENT_TYPE, mime);
    }
    let disposition = format!(
        "inline; filename*=UTF-8''{}",
        utf8_percent_encode(&file.original_name, NON_ALPHANUMERIC)
    );
    if let Ok(value) = HeaderValue::from_str(&disposition) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    Ok(response)
}

pub async fn delete(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path((product_id, file_id)): Path<(i64, i64)>,
    jar: CookieJar,
) -> AppResult<Response> {
    let language = manuals::file_language(&state.db, product_id, file_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if let Some(stored_name) = manuals::delete_file(&state.db, file_id).await? {
        storage::remove_files(&state.config.upload_dir, &[stored_name]).await;
    }
    tracing::info!(product_id, file_id, by = %user.email, "изтрит файл");
    let jar = Flash::ok("Файлът е изтрит.").set(jar);
    Ok((jar, Redirect::to(&manual_url(product_id, &language))).into_response())
}

#[derive(Deserialize)]
pub struct MoveForm {
    direction: String,
}

pub async fn reorder(
    State(state): State<AppState>,
    CurrentUser(_): CurrentUser,
    Path((product_id, file_id)): Path<(i64, i64)>,
    Form(form): Form<MoveForm>,
) -> AppResult<impl IntoResponse> {
    let language = manuals::file_language(&state.db, product_id, file_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let up = match form.direction.as_str() {
        "up" => true,
        "down" => false,
        _ => return Err(AppError::Status(StatusCode::BAD_REQUEST, "Невалидна посока".into())),
    };
    manuals::move_file(&state.db, file_id, up).await?;
    Ok(Redirect::to(&manual_url(product_id, &language)))
}
