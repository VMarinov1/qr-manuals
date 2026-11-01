mod auth;
mod error;

use std::sync::Arc;

use askama::Template;
use axum::{
    extract::{Request, State},
    http::{header, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
    Router,
};
use sqlx::SqlitePool;
use tokio::net::TcpListener;
use tower_http::{services::ServeDir, trace::TraceLayer};

use crate::config::Config;
use auth::CurrentUser;
use error::{render, AppResult};

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub config: Arc<Config>,
}

pub async fn serve(config: Config, db: SqlitePool) -> anyhow::Result<()> {
    tokio::fs::create_dir_all(&config.upload_dir).await?;

    let addr = config.bind_addr;
    let static_dir = ServeDir::new(&config.static_dir);
    let state = AppState {
        db,
        config: Arc::new(config),
    };

    let app = Router::new()
        .route("/", get(|| async { Redirect::to("/products") }))
        .route("/login", get(auth::login_page).post(auth::login_submit))
        .route("/logout", post(auth::logout))
        .route("/products", get(products))
        .route("/health", get(health))
        .nest_service("/static", static_dir)
        .layer(middleware::from_fn_with_state(state.clone(), check_origin))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = TcpListener::bind(addr).await?;
    tracing::info!("слуша на http://{addr}");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

/// Защита от CSRF: заявки, които променят данни, се приемат само от нашия сайт.
/// Бисквитката е и SameSite=Lax, това е втори слой.
async fn check_origin(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let safe = matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    if !safe {
        if let Some(origin) = req.headers().get(header::ORIGIN) {
            let origin = origin.to_str().unwrap_or_default();
            let host = req
                .headers()
                .get(header::HOST)
                .and_then(|h| h.to_str().ok())
                .unwrap_or_default();
            let same_host = origin
                .split_once("://")
                .is_some_and(|(_, rest)| rest == host);
            if !same_host && origin != state.config.public_base_url {
                tracing::warn!(%origin, %host, "отхвърлена заявка от чужд произход");
                return (StatusCode::FORBIDDEN, "Забранено").into_response();
            }
        }
    }
    next.run(req).await
}

#[derive(Template)]
#[template(path = "products.html")]
struct ProductsTemplate {
    user: crate::auth::User,
    product_count: i64,
}

// Временна страница, докато няма списък с продукти.
async fn products(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
) -> AppResult<impl IntoResponse> {
    let product_count = sqlx::query_scalar("SELECT COUNT(*) FROM products")
        .fetch_one(&state.db)
        .await?;
    render(&ProductsTemplate {
        user,
        product_count,
    })
}

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    match sqlx::query("SELECT 1").execute(&state.db).await {
        Ok(_) => (StatusCode::OK, "ok"),
        Err(err) => {
            tracing::error!("health: {err}");
            (StatusCode::SERVICE_UNAVAILABLE, "db unavailable")
        }
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.ok();
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            sig.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("спиране");
}
