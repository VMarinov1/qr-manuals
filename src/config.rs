use std::{net::SocketAddr, path::PathBuf};

use anyhow::Context;

#[derive(Debug, Clone)]
pub struct Config {
    pub bind_addr: SocketAddr,
    pub database_url: String,
    pub upload_dir: PathBuf,
    pub static_dir: PathBuf,
    /// Адресът, към който сочат публичните линкове и QR кодовете, без `/` накрая.
    pub public_base_url: String,
    pub max_upload_bytes: usize,
    /// Таван на една заявка за качване (всички файлове заедно). Cloudflare спира над 100 MB.
    pub max_request_bytes: usize,
    /// Приложението е зад reverse proxy и адресът на клиента е в `X-Forwarded-For`.
    pub trust_proxy: bool,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let max_upload_mb: usize = var("MAX_UPLOAD_MB", "20")
            .parse()
            .context("MAX_UPLOAD_MB трябва да е цяло число")?;
        let max_request_mb: usize = var("MAX_REQUEST_MB", "95")
            .parse()
            .context("MAX_REQUEST_MB трябва да е цяло число")?;

        Ok(Self {
            bind_addr: var("BIND_ADDR", "0.0.0.0:8080")
                .parse()
                .context("невалиден BIND_ADDR")?,
            database_url: var("DATABASE_URL", "sqlite://data/app.db"),
            upload_dir: var("UPLOAD_DIR", "data/uploads").into(),
            static_dir: var("STATIC_DIR", "static").into(),
            public_base_url: var("PUBLIC_BASE_URL", "http://localhost:8080")
                .trim_end_matches('/')
                .to_owned(),
            max_upload_bytes: max_upload_mb * 1024 * 1024,
            max_request_bytes: max_request_mb * 1024 * 1024,
            trust_proxy: matches!(
                var("TRUST_PROXY", "false").to_ascii_lowercase().as_str(),
                "1" | "true" | "yes"
            ),
        })
    }
}

fn var(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_owned())
}
