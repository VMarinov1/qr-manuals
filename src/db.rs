use std::{str::FromStr, time::Duration};

use anyhow::Context;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};

/// Отваря базата (създава я, ако липсва) и пуска миграциите.
pub async fn connect(url: &str) -> anyhow::Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str(url)
        .with_context(|| format!("невалиден DATABASE_URL: {url}"))?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));

    if let Some(dir) = options.get_filename().parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("не може да се създаде {}", dir.display()))?;
        }
    }

    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await
        .context("не може да се отвори базата")?;

    sqlx::migrate!()
        .run(&pool)
        .await
        .context("миграциите не минаха")?;

    Ok(pool)
}
