use askama::Template;
use axum::{
    extract::{FromRequestParts, Query, State},
    http::{request::Parts, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Form,
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::Deserialize;

use super::{
    error::{render, AppError, AppResult},
    AppState,
};
use crate::auth::{self, User, SESSION_DAYS};

const SESSION_COOKIE: &str = "qrm_session";
const HOME: &str = "/products";

/// Влезлият потребител. Handler, който го приема, е достъпен само след вход.
pub struct CurrentUser(pub User);

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Response> {
        let jar = CookieJar::from_headers(&parts.headers);
        match session_user(state, &jar).await {
            Ok(Some(user)) => Ok(CurrentUser(user)),
            Ok(None) => Err(to_login(parts)),
            Err(err) => Err(err.into_response()),
        }
    }
}

async fn session_user(state: &AppState, jar: &CookieJar) -> AppResult<Option<User>> {
    match jar.get(SESSION_COOKIE) {
        Some(cookie) => Ok(auth::user_for_session(&state.db, cookie.value()).await?),
        None => Ok(None),
    }
}

fn to_login(parts: &Parts) -> Response {
    let target = format!("/login?next={}", encode_path(parts.uri.path()));
    // HTMX следва обикновения redirect вътре в страницата, затова му се казва изрично.
    if parts.headers.contains_key("hx-request") {
        let mut response = StatusCode::OK.into_response();
        if let Ok(value) = HeaderValue::from_str(&target) {
            response.headers_mut().insert("hx-redirect", value);
        }
        return response;
    }
    Redirect::to(&target).into_response()
}

#[derive(Template)]
#[template(path = "login.html")]
struct LoginTemplate {
    email: String,
    next: String,
    error: Option<&'static str>,
}

#[derive(Deserialize)]
pub struct LoginQuery {
    next: Option<String>,
}

pub async fn login_page(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<LoginQuery>,
) -> AppResult<Response> {
    let next = safe_next(query.next.as_deref());
    if session_user(&state, &jar).await?.is_some() {
        return Ok(Redirect::to(next).into_response());
    }
    Ok(render(&LoginTemplate {
        email: String::new(),
        next: next.to_owned(),
        error: None,
    })?
    .into_response())
}

#[derive(Deserialize)]
pub struct LoginForm {
    email: String,
    password: String,
    next: Option<String>,
}

pub async fn login_submit(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<LoginForm>,
) -> AppResult<Response> {
    let next = safe_next(form.next.as_deref()).to_owned();

    let Some(user) = auth::authenticate(&state.db, &form.email, &form.password).await? else {
        tracing::warn!(email = %form.email.trim(), "неуспешен вход");
        let page = render(&LoginTemplate {
            email: form.email.trim().to_owned(),
            next,
            error: Some("Грешен email или парола."),
        })?;
        return Ok((StatusCode::UNAUTHORIZED, page).into_response());
    };

    let token = auth::create_session(&state.db, user.id).await?;
    let cookie = Cookie::build((SESSION_COOKIE, token))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(state.config.public_base_url.starts_with("https://"))
        .max_age(time::Duration::days(SESSION_DAYS));

    tracing::info!(email = %user.email, "вход");
    Ok((jar.add(cookie), Redirect::to(&next)).into_response())
}

pub async fn logout(State(state): State<AppState>, jar: CookieJar) -> Result<Response, AppError> {
    if let Some(cookie) = jar.get(SESSION_COOKIE) {
        auth::delete_session(&state.db, cookie.value()).await?;
    }
    let jar = jar.remove(Cookie::build(SESSION_COOKIE).path("/"));
    Ok((jar, Redirect::to("/login")).into_response())
}

/// Позволява връщане само към локален път, за да не може линк да прати потребителя другаде.
fn safe_next(next: Option<&str>) -> &str {
    match next {
        Some(path) if path.starts_with('/') && !path.starts_with("//") && !path.contains('\\') => {
            path
        }
        _ => HOME,
    }
}

fn encode_path(path: &str) -> String {
    path.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_only_allows_local_paths() {
        assert_eq!(safe_next(Some("/products/5")), "/products/5");
        assert_eq!(safe_next(Some("//evil.com")), HOME);
        assert_eq!(safe_next(Some("/\\evil.com")), HOME);
        assert_eq!(safe_next(Some("https://evil.com")), HOME);
        assert_eq!(safe_next(None), HOME);
    }

    #[test]
    fn encodes_non_path_characters() {
        assert_eq!(encode_path("/products/5"), "/products/5");
        assert_eq!(encode_path("/a b&c"), "/a%20b%26c");
    }
}
