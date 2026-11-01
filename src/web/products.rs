use askama::Template;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
    Form,
};
use serde::Deserialize;

use super::{
    auth::CurrentUser,
    error::{render, AppError, AppResult},
    AppState,
};
use crate::{
    auth::User,
    products::{
        self, display_date, FormErrors, Language, Product, ProductForm, ProductListItem,
        SaveError,
    },
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

#[derive(Template)]
#[template(path = "products/show.html")]
struct ShowTemplate {
    user: User,
    product: Product,
    public_url: String,
    languages: Vec<Language>,
    form: ProductForm,
    errors: FormErrors,
    saved: bool,
}

impl ShowTemplate {
    fn created_date(&self) -> String {
        display_date(&self.product.created_at)
    }
}

#[derive(Deserialize)]
pub struct ShowQuery {
    #[serde(default)]
    saved: Option<String>,
}

pub async fn show(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<i64>,
    Query(query): Query<ShowQuery>,
) -> AppResult<impl IntoResponse> {
    let product = products::get(&state.db, id).await?.ok_or(AppError::NotFound)?;
    render(&ShowTemplate {
        user,
        public_url: public_url(&state, &product),
        languages: products::languages(&state.db).await?,
        form: ProductForm::from(&product),
        errors: FormErrors::default(),
        saved: query.saved.is_some(),
        product,
    })
}

pub async fn update(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<i64>,
    Form(form): Form<ProductForm>,
) -> AppResult<Response> {
    let product = products::get(&state.db, id).await?.ok_or(AppError::NotFound)?;
    let languages = products::languages(&state.db).await?;
    let errors = match form.validate(&languages) {
        Ok(input) => match products::update(&state.db, id, &input).await {
            Ok(true) => {
                tracing::info!(id, by = %user.email, "редактиран продукт");
                return Ok(Redirect::to(&format!("/products/{id}?saved=1")).into_response());
            }
            Ok(false) => return Err(AppError::NotFound),
            Err(err) => save_error(err)?,
        },
        Err(errors) => errors,
    };
    let page = render(&ShowTemplate {
        user,
        public_url: public_url(&state, &product),
        languages,
        form,
        errors,
        saved: false,
        product,
    })?;
    Ok((StatusCode::UNPROCESSABLE_ENTITY, page).into_response())
}

pub async fn delete(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<i64>,
) -> AppResult<Response> {
    if !products::delete(&state.db, id).await? {
        return Err(AppError::NotFound);
    }
    tracing::info!(id, by = %user.email, "изтрит продукт");
    Ok(Redirect::to("/products").into_response())
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

fn public_url(state: &AppState, product: &Product) -> String {
    format!("{}/m/{}", state.config.public_base_url, product.public_slug)
}
