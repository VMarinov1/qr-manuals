use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};

/// Грешка в handler. Логва се пълната причина, а потребителят вижда общо съобщение.
pub struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        tracing::error!("{:#}", self.0);
        (StatusCode::INTERNAL_SERVER_ERROR, "Възникна грешка").into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

pub type AppResult<T> = Result<T, AppError>;

pub fn render<T: askama::Template>(template: &T) -> AppResult<Html<String>> {
    Ok(Html(template.render()?))
}
