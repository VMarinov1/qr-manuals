use std::sync::OnceLock;

use anyhow::anyhow;
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use rand_core::RngCore;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

pub const MIN_PASSWORD_LEN: usize = 8;
pub const SESSION_DAYS: i64 = 30;

const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%SZ', 'now')";

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct User {
    pub id: i64,
    pub email: String,
}

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|err| anyhow!("хеширане на парола: {err}"))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        })
        .unwrap_or(false)
}

/// Проверява email и парола. При непознат email също се изчислява хеш,
/// за да не може по времето за отговор да се разбере кои адреси съществуват.
pub async fn authenticate(
    db: &SqlitePool,
    email: &str,
    password: &str,
) -> anyhow::Result<Option<User>> {
    let row: Option<(i64, String, String)> =
        sqlx::query_as("SELECT id, email, password_hash FROM users WHERE email = ?")
            .bind(email.trim())
            .fetch_optional(db)
            .await?;

    Ok(match row {
        Some((id, email, hash)) if verify_password(password, &hash) => Some(User { id, email }),
        Some(_) => None,
        None => {
            verify_password(password, dummy_hash());
            None
        }
    })
}

fn dummy_hash() -> &'static str {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| hash_password("timing-equalizer").expect("argon2"))
}

/// Създава сесия и връща токена за бисквитката. В базата се пази само хешът му.
pub async fn create_session(db: &SqlitePool, user_id: i64) -> anyhow::Result<String> {
    sqlx::query(&format!("DELETE FROM sessions WHERE expires_at <= {NOW}"))
        .execute(db)
        .await?;

    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let token = to_hex(&bytes);

    sqlx::query(
        "INSERT INTO sessions (id, user_id, expires_at)
         VALUES (?, ?, strftime('%Y-%m-%dT%H:%M:%SZ', 'now', ?))",
    )
    .bind(session_id(&token))
    .bind(user_id)
    .bind(format!("+{SESSION_DAYS} days"))
    .execute(db)
    .await?;

    Ok(token)
}

pub async fn user_for_session(db: &SqlitePool, token: &str) -> anyhow::Result<Option<User>> {
    let user = sqlx::query_as(&format!(
        "SELECT u.id, u.email FROM sessions s JOIN users u ON u.id = s.user_id
         WHERE s.id = ? AND s.expires_at > {NOW}"
    ))
    .bind(session_id(token))
    .fetch_optional(db)
    .await?;
    Ok(user)
}

pub async fn delete_session(db: &SqlitePool, token: &str) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM sessions WHERE id = ?")
        .bind(session_id(token))
        .execute(db)
        .await?;
    Ok(())
}

fn session_id(token: &str) -> String {
    to_hex(&Sha256::digest(token.as_bytes()))
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Създава потребител или сменя паролата му, ако вече съществува. Връща true при нов.
pub async fn upsert_user(db: &SqlitePool, email: &str, password: &str) -> anyhow::Result<bool> {
    let email = email.trim();
    if !email.contains('@') {
        anyhow::bail!("невалиден email: {email}");
    }
    if password.chars().count() < MIN_PASSWORD_LEN {
        anyhow::bail!("паролата трябва да е поне {MIN_PASSWORD_LEN} символа");
    }

    let hash = hash_password(password)?;
    let existed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE email = ?)")
        .bind(email)
        .fetch_one(db)
        .await?;

    sqlx::query(
        "INSERT INTO users (email, password_hash) VALUES (?, ?)
         ON CONFLICT (email) DO UPDATE SET password_hash = excluded.password_hash",
    )
    .bind(email)
    .bind(&hash)
    .execute(db)
    .await?;

    if existed {
        // Смяна на парола прекратява всички отворени сесии.
        sqlx::query("DELETE FROM sessions WHERE user_id = (SELECT id FROM users WHERE email = ?)")
            .bind(email)
            .execute(db)
            .await?;
    }

    Ok(!existed)
}
