use std::{
    path::{Path, PathBuf},
    str::FromStr,
    time::Duration,
};

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

/// Съгласувано копие на базата, докато приложението работи (`VACUUM INTO`).
/// Пази последните `keep` копия в папката и връща пътя на новото.
pub async fn backup(pool: &SqlitePool, dir: &Path, keep: usize) -> anyhow::Result<PathBuf> {
    tokio::fs::create_dir_all(dir)
        .await
        .with_context(|| format!("не може да се създаде {}", dir.display()))?;
    let stamp: String = sqlx::query_scalar("SELECT strftime('%Y%m%d-%H%M%S', 'now')")
        .fetch_one(pool)
        .await?;
    let path = dir.join(format!("app-{stamp}.db"));
    // VACUUM INTO не презаписва. Копие от същата секунда е също толкова ново.
    match tokio::fs::remove_file(&path).await {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => return Err(err.into()),
        _ => {}
    }
    sqlx::query("VACUUM INTO ?")
        .bind(path.to_string_lossy().as_ref())
        .execute(pool)
        .await
        .context("копието на базата не мина")?;

    // Имената съдържат датата, затова подредбата по име е и по време.
    let mut existing = Vec::new();
    let mut entries = tokio::fs::read_dir(dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("app-") && name.ends_with(".db") {
            existing.push(entry.path());
        }
    }
    existing.sort();
    let excess = existing.len().saturating_sub(keep.max(1));
    for old in &existing[..excess] {
        tokio::fs::remove_file(old).await.ok();
    }
    Ok(path)
}
