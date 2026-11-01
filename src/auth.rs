use anyhow::anyhow;
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use sqlx::SqlitePool;

pub const MIN_PASSWORD_LEN: usize = 8;

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|err| anyhow!("хеширане на парола: {err}"))
}

#[allow(dead_code)] // ще се ползва при входа
pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        })
        .unwrap_or(false)
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
