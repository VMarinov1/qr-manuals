use askama::Template;
use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Form,
};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;

use super::{
    auth::CurrentUser,
    error::{render, AppError, AppResult},
    files::MAX_FILES_PER_UPLOAD,
    flash::Flash,
    AppState,
};
use crate::{
    auth::User,
    manuals::{self, FileRow},
    products::{
        self, display_date, FormErrors, Language, Product, ProductForm, ProductListItem,
        SaveError,
    },
    qr, storage,
};

#[derive(Template)]
#[template(path = "products/list.html")]
struct ListTemplate {
    user: User,
    search: String,
    products: Vec<ProductListItem>,
}

#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    q: String,
}

pub async fn list(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Query(query): Query<ListQuery>,
) -> AppResult<impl IntoResponse> {
    let products = products::list(&state.db, &query.q).await?;
    render(&ListTemplate {
        user,
        search: query.q,
        products,
    })
}

#[derive(Template)]
#[template(path = "products/new.html")]
struct NewTemplate {
    user: User,
    languages: Vec<Language>,
    form: ProductForm,
    errors: FormErrors,
}

pub async fn new_page(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
) -> AppResult<impl IntoResponse> {
    render(&NewTemplate {
        user,
        languages: products::languages(&state.db).await?,
        form: ProductForm {
            default_language: "bg".into(),
            ..Default::default()
        },
        errors: FormErrors::default(),
    })
}

pub async fn create(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Form(form): Form<ProductForm>,
) -> AppResult<Response> {
    let languages = products::languages(&state.db).await?;
    let errors = match form.validate(&languages) {
        Ok(input) => match products::create(&state.db, &input).await {
            Ok(id) => {
                tracing::info!(id, by = %user.email, "нов продукт");
                return Ok(Redirect::to(&format!("/products/{id}")).into_response());
            }
            Err(err) => save_error(err)?,
        },
        Err(errors) => errors,
    };
    let page = render(&NewTemplate {
        user,
        languages,
        form,
        errors,
    })?;
    Ok((StatusCode::UNPROCESSABLE_ENTITY, page).into_response())
}

struct LanguageTab {
    code: String,
    name: String,
    file_count: i64,
    active: bool,
}

#[derive(Template)]
#[template(path = "products/show.html")]
struct ShowTemplate {
    user: User,
    product: Product,
    public_url: String,
    languages: Vec<Language>,
    form: ProductForm,
    errors: FormErrors,
    flash: Option<Flash>,
    tabs: Vec<LanguageTab>,
    files: Vec<FileRow>,
    max_upload_mb: usize,
    max_request_mb: usize,
    max_files: usize,
}

impl ShowTemplate {
    fn created_date(&self) -> String {
        display_date(&self.product.created_at)
    }

    fn active_tab(&self) -> &LanguageTab {
        self.tabs.iter().find(|t| t.active).unwrap_or(&self.tabs[0])
    }
}

/// Сглобява страницата на продукта. Активният език е поисканият, иначе първият с
/// упътване, иначе езикът по подразбиране.
async fn show_page(
    state: &AppState,
    user: User,
    product: Product,
    form: ProductForm,
    errors: FormErrors,
    language: Option<&str>,
    flash: Option<Flash>,
) -> AppResult<ShowTemplate> {
    let languages = products::languages(&state.db).await?;
    let counts = manuals::file_counts(&state.db, product.id).await?;
    let count_for = |code: &str| {
        counts
            .iter()
            .find(|(c, _)| c == code)
            .map_or(0, |(_, n)| *n)
    };

    let active = language
        .filter(|code| languages.iter().any(|l| l.code == *code))
        .or_else(|| {
            languages
                .iter()
                .find(|l| count_for(&l.code) > 0)
                .map(|l| l.code.as_str())
        })
        .unwrap_or(&product.default_language)
        .to_owned();

    let tabs = languages
        .iter()
        .map(|l| LanguageTab {
            code: l.code.clone(),
            name: l.name.clone(),
            file_count: count_for(&l.code),
            active: l.code == active,
        })
        .collect();

    Ok(ShowTemplate {
        user,
        public_url: public_url(state, &product),
        files: manuals::files(&state.db, product.id, &active).await?,
        product,
        languages,
        form,
        errors,
        flash,
        tabs,
        max_upload_mb: state.config.max_upload_bytes / 1024 / 1024,
        max_request_mb: state.config.max_request_bytes / 1024 / 1024,
        max_files: MAX_FILES_PER_UPLOAD,
    })
}

#[derive(Deserialize)]
pub struct ShowQuery {
    lang: Option<String>,
}

pub async fn show(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<i64>,
    Query(query): Query<ShowQuery>,
    jar: CookieJar,
) -> AppResult<Response> {
    let product = products::get(&state.db, id).await?.ok_or(AppError::NotFound)?;
    let (jar, flash) = Flash::take(jar);
    let form = ProductForm::from(&product);
    let page = show_page(
        &state,
        user,
        product,
        form,
        FormErrors::default(),
        query.lang.as_deref(),
        flash,
    )
    .await?;
    Ok((jar, render(&page)?).into_response())
}

pub async fn update(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<i64>,
    jar: CookieJar,
    Form(form): Form<ProductForm>,
) -> AppResult<Response> {
    let product = products::get(&state.db, id).await?.ok_or(AppError::NotFound)?;
    let languages = products::languages(&state.db).await?;
    let errors = match form.validate(&languages) {
        Ok(input) => match products::update(&state.db, id, &input).await {
            Ok(true) => {
                tracing::info!(id, by = %user.email, "редактиран продукт");
                let jar = Flash::ok("Промените са запазени.").set(jar);
                return Ok((jar, Redirect::to(&format!("/products/{id}"))).into_response());
            }
            Ok(false) => return Err(AppError::NotFound),
            Err(err) => save_error(err)?,
        },
        Err(errors) => errors,
    };
    let page = show_page(&state, user, product, form, errors, None, None).await?;
    Ok((StatusCode::UNPROCESSABLE_ENTITY, render(&page)?).into_response())
}

pub async fn delete(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<i64>,
) -> AppResult<Response> {
    let stored_names = manuals::stored_names_for_product(&state.db, id).await?;
    if !products::delete(&state.db, id).await? {
        return Err(AppError::NotFound);
    }
    storage::remove_files(&state.config.upload_dir, &stored_names).await;
    tracing::info!(id, files = stored_names.len(), by = %user.email, "изтрит продукт");
    Ok(Redirect::to("/products").into_response())
}

#[derive(Deserialize)]
pub struct QrQuery {
    #[serde(default)]
    download: Option<String>,
}

pub async fn qr_svg(
    State(state): State<AppState>,
    CurrentUser(_): CurrentUser,
    Path(id): Path<i64>,
    Query(query): Query<QrQuery>,
) -> AppResult<Response> {
    let product = products::get(&state.db, id).await?.ok_or(AppError::NotFound)?;
    let svg = qr::svg(&public_url(&state, &product))?;
    Ok(qr_response(&product, "svg", "image/svg+xml", svg.into_bytes(), query.download.is_some()))
}

pub async fn qr_png(
    State(state): State<AppState>,
    CurrentUser(_): CurrentUser,
    Path(id): Path<i64>,
    Query(query): Query<QrQuery>,
) -> AppResult<Response> {
    let product = products::get(&state.db, id).await?.ok_or(AppError::NotFound)?;
    // ~10 см при 300 DPI.
    let png = qr::png(&public_url(&state, &product), 1200)?;
    Ok(qr_response(&product, "png", "image/png", png, query.download.is_some()))
}

fn qr_response(product: &Product, ext: &str, mime: &'static str, body: Vec<u8>, download: bool) -> Response {
    // Името на файла е по SKU, ако има, за да се намира лесно при печат.
    let base: String = product
        .sku
        .as_deref()
        .unwrap_or(&product.public_slug)
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let disposition = format!(
        "{}; filename=\"qr-{base}.{ext}\"",
        if download { "attachment" } else { "inline" }
    );
    (
        [
            (header::CONTENT_TYPE, mime.to_owned()),
            (header::CONTENT_DISPOSITION, disposition),
            (header::CACHE_CONTROL, "private, no-cache".to_owned()),
        ],
        body,
    )
        .into_response()
}

fn public_url(state: &AppState, product: &Product) -> String {
    format!("{}/m/{}", state.config.public_base_url, product.public_slug)
}

/// Грешка при запис, която потребителят може да поправи, става грешка в полето.
fn save_error(err: SaveError) -> AppResult<FormErrors> {
    match err {
        SaveError::DuplicateSku => Ok(FormErrors {
            sku: Some("Вече има продукт с този SKU."),
            ..Default::default()
        }),
        SaveError::Other(err) => Err(err.into()),
    }
}
