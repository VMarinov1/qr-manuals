mod auth;
mod error;
mod files;
mod flash;
mod products;
mod public;
mod rate_limit;

use std::{net::SocketAddr, sync::Arc, time::Duration};

use anyhow::Context;
use axum::{
    extract::{DefaultBodyLimit, Request, State},
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

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub config: Arc<Config>,
    pub login_limiter: Arc<rate_limit::LoginLimiter>,
}

pub async fn serve(config: Config, db: SqlitePool) -> anyhow::Result<()> {
    tokio::fs::create_dir_all(&config.upload_dir).await?;
    crate::storage::remove_temp_files(&config.upload_dir).await?;

    let addr = config.bind_addr;
    let static_dir = ServeDir::new(&config.static_dir);
    // Лимитът на файл се проверява при четенето, това е таванът за цялата заявка.
    let upload_limit = config.max_upload_bytes * files::MAX_FILES_PER_UPLOAD + 1024 * 1024;
    let state = AppState {
        db,
        config: Arc::new(config),
        login_limiter: Arc::default(),
    };

    let app = Router::new()
        .route("/", get(|| async { Redirect::to("/products") }))
        .route("/login", get(auth::login_page).post(auth::login_submit))
        .route("/logout", post(auth::logout))
        .route("/products", get(products::list).post(products::create))
        .route("/products/new", get(products::new_page))
        .route("/products/{id}", get(products::show).post(products::update))
        .route("/products/{id}/delete", post(products::delete))
        .route(
            "/products/{id}/manuals/{lang}/files",
            post(files::upload).layer(DefaultBodyLimit::max(upload_limit)),
        )
        .route("/products/{id}/files/{file_id}/move", post(files::reorder))
        .route("/products/{id}/files/{file_id}/delete", post(files::delete))
        .route("/products/{id}/qr.svg", get(products::qr_svg))
        .route("/products/{id}/qr.png", get(products::qr_png))
        .route("/f/{stored_name}", get(files::serve))
        .route("/m/{slug}", get(public::manual))
        .route("/m/{slug}/{lang}", get(public::manual_in_language))
        .route("/health", get(health))
        .nest_service("/static", static_dir)
        .layer(middleware::from_fn_with_state(state.clone(), check_origin))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = TcpListener::bind(addr).await?;
    tracing::info!("слуша на http://{addr}");
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
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

/// Healthcheck за Docker: GET /health към собствения порт.
pub async fn health_check(port: u16) -> anyhow::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let check = async {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await?;
        stream
            .write_all(b"GET /health HTTP/1.0\r\nHost: localhost\r\n\r\n")
            .await?;
        let mut head = [0u8; 12];
        stream.read_exact(&mut head).await?;
        anyhow::ensure!(head.ends_with(b" 200"), "отговор {}", String::from_utf8_lossy(&head));
        Ok(())
    };
    tokio::time::timeout(Duration::from_secs(5), check)
        .await
        .context("няма отговор")?
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
