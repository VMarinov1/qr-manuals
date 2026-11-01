//! Публичната страница, която се отваря от QR кода. Без текст на интерфейса:
//! само съдържанието на упътването, иконки и кодове на езици.

use askama::Template;
use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
};

use super::{
    error::{render, AppResult},
    AppState,
};
use crate::{
    language,
    manuals::{self, FileRow},
    products::{self, Product},
};

#[derive(Template)]
#[template(path = "public/manual.html")]
struct ManualTemplate {
    product: Product,
    /// Кодът на показания език, или празен, ако няма упътвания.
    language: String,
    languages: Vec<String>,
    files: Vec<FileRow>,
}

#[derive(Template)]
#[template(path = "public/not_found.html")]
struct NotFoundTemplate;

pub async fn manual(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> AppResult<Response> {
    respond(&state, &slug, None, &headers).await
}

pub async fn manual_in_language(
    State(state): State<AppState>,
    Path((slug, language)): Path<(String, String)>,
    headers: HeaderMap,
) -> AppResult<Response> {
    respond(&state, &slug, Some(&language), &headers).await
}

async fn respond(
    state: &AppState,
    slug: &str,
    requested: Option<&str>,
    headers: &HeaderMap,
) -> AppResult<Response> {
    let product = if products::is_valid_slug(slug) {
        products::get_by_slug(&state.db, slug).await?
    } else {
        None
    };
    let Some(product) = product else {
        return Ok((StatusCode::NOT_FOUND, render(&NotFoundTemplate)?).into_response());
    };

    let available = manuals::languages_with_files(&state.db, product.id).await?;

    let chosen = match requested {
        Some(code) if available.iter().any(|a| a == code) => Some(code.to_owned()),
        // Няма упътване на поискания език: към общия линк, който избира сам.
        Some(_) => return Ok(Redirect::to(&format!("/m/{slug}")).into_response()),
        None => {
            let accept = headers
                .get(header::ACCEPT_LANGUAGE)
                .and_then(|v| v.to_str().ok());
            language::choose(accept, &product.default_language, &available).map(str::to_owned)
        }
    };

    let files = match &chosen {
        Some(code) => manuals::files(&state.db, product.id, code).await?,
        None => Vec::new(),
    };

    let page = render(&ManualTemplate {
        product,
        language: chosen.unwrap_or_default(),
        languages: available,
        files,
    })?;

    let mut response = page.into_response();
    let headers = response.headers_mut();
    // Отговорът зависи от езика на телефона, а упътването може да се смени по всяко време.
    headers.insert(header::VARY, HeaderValue::from_static("Accept-Language"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    Ok(response)
}
