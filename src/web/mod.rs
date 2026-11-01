mod error;

use std::sync::Arc;

use askama::Template;
use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Router};
use sqlx::SqlitePool;
use tokio::net::TcpListener;
use tower_http::{services::ServeDir, trace::TraceLayer};

use crate::config::Config;
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
        .route("/", get(index))
        .route("/health", get(health))
        .nest_service("/static", static_dir)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = TcpListener::bind(addr).await?;
    tracing::info!("слуша на http://{addr}");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    product_count: i64,
}

// Временна начална страница, докато няма вход и списък с продукти.
async fn index(State(state): State<AppState>) -> AppResult<impl IntoResponse> {
    let product_count = sqlx::query_scalar("SELECT COUNT(*) FROM products")
        .fetch_one(&state.db)
        .await?;
    render(&IndexTemplate { product_count })
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
