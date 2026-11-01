use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};

pub enum AppError {
    NotFound,
    /// Логва се пълната причина, а потребителят вижда общо съобщение.
    Internal(anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            AppError::NotFound => (StatusCode::NOT_FOUND, "Не е намерено").into_response(),
            AppError::Internal(err) => {
                tracing::error!("{err:#}");
                (StatusCode::INTERNAL_SERVER_ERROR, "Възникна грешка").into_response()
            }
        }
    }
}

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(err: E) -> Self {
        Self::Internal(err.into())
    }
}

pub type AppResult<T> = Result<T, AppError>;

pub fn render<T: askama::Template>(template: &T) -> AppResult<Html<String>> {
    Ok(Html(template.render()?))
}
