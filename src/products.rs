use argon2::password_hash::rand_core::{OsRng, RngCore};
use sqlx::SqlitePool;

pub const NAME_MAX: usize = 200;
pub const SKU_MAX: usize = 64;
pub const DESCRIPTION_MAX: usize = 2000;

/// Без 0/O, 1/l/I, за да се чете лесно, ако някой препише линка от етикета.
const SLUG_ALPHABET: &[u8] = b"23456789abcdefghjkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ";
const SLUG_LEN: usize = 8;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Language {
    pub code: String,
    pub name: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Product {
    pub id: i64,
    pub name: String,
    pub sku: Option<String>,
    pub description: String,
    pub public_slug: String,
    pub default_language: String,
    pub created_at: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ProductListItem {
    pub id: i64,
    pub name: String,
    pub sku: Option<String>,
    /// Кодовете на езиците с упътване, разделени със запетая.
    pub languages: Option<String>,
    pub created_at: String,
}

impl ProductListItem {
    pub fn language_codes(&self) -> Vec<&str> {
        self.languages
            .as_deref()
            .map(|codes| codes.split(',').collect())
            .unwrap_or_default()
    }

    pub fn created_date(&self) -> String {
        display_date(&self.created_at)
    }
}

/// `2026-11-01T19:17:33Z` -> `01.11.2026`
pub fn display_date(timestamp: &str) -> String {
    match (timestamp.get(0..4), timestamp.get(5..7), timestamp.get(8..10)) {
        (Some(y), Some(m), Some(d)) => format!("{d}.{m}.{y}"),
        _ => timestamp.to_owned(),
    }
}

/// Данните от формата, както са въведени. Пазят се и при грешка, за да се покажат отново.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct ProductForm {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub sku: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub default_language: String,
}

impl From<&Product> for ProductForm {
    fn from(product: &Product) -> Self {
        Self {
            name: product.name.clone(),
            sku: product.sku.clone().unwrap_or_default(),
            description: product.description.clone(),
            default_language: product.default_language.clone(),
        }
    }
}

#[derive(Debug, Default)]
pub struct FormErrors {
    pub name: Option<&'static str>,
    pub sku: Option<&'static str>,
    pub default_language: Option<&'static str>,
    pub description: Option<&'static str>,
}

impl FormErrors {
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.sku.is_none()
            && self.default_language.is_none()
            && self.description.is_none()
    }
}

/// Проверени данни, готови за запис.
pub struct ProductInput {
    name: String,
    sku: Option<String>,
    description: String,
    default_language: String,
}

impl ProductInput {
    pub fn search_text(&self) -> String {
        format!("{} {}", self.name, self.sku.as_deref().unwrap_or("")).to_lowercase()
    }
}

impl ProductForm {
    pub fn validate(&self, languages: &[Language]) -> Result<ProductInput, FormErrors> {
        let mut errors = FormErrors::default();
        let name = self.name.trim();
        let sku = self.sku.trim();
        let description = self.description.trim();

        if name.is_empty() {
            errors.name = Some("Въведи име.");
        } else if name.chars().count() > NAME_MAX {
            errors.name = Some("Името е твърде дълго.");
        }
        if sku.chars().count() > SKU_MAX {
            errors.sku = Some("SKU е твърде дълъг.");
        }
        if description.chars().count() > DESCRIPTION_MAX {
            errors.description = Some("Описанието е твърде дълго.");
        }
        if !languages.iter().any(|l| l.code == self.default_language) {
            errors.default_language = Some("Избери език.");
        }

        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(ProductInput {
            name: name.to_owned(),
            sku: (!sku.is_empty()).then(|| sku.to_owned()),
            description: description.to_owned(),
            default_language: self.default_language.clone(),
        })
    }
}

pub enum SaveError {
    DuplicateSku,
    Other(anyhow::Error),
}

impl From<sqlx::Error> for SaveError {
    fn from(err: sqlx::Error) -> Self {
        if is_unique_violation(&err, "products.sku") {
            SaveError::DuplicateSku
        } else {
            SaveError::Other(err.into())
        }
    }
}

fn is_unique_violation(err: &sqlx::Error, column: &str) -> bool {
    err.as_database_error()
        .is_some_and(|db| db.is_unique_violation() && db.message().contains(column))
}

pub async fn languages(db: &SqlitePool) -> anyhow::Result<Vec<Language>> {
    Ok(
        sqlx::query_as("SELECT code, name FROM languages ORDER BY sort_order")
            .fetch_all(db)
            .await?,
    )
}

pub async fn list(db: &SqlitePool, search: &str) -> anyhow::Result<Vec<ProductListItem>> {
    let pattern = format!("%{}%", escape_like(&search.trim().to_lowercase()));
    Ok(sqlx::query_as(
        r"SELECT p.id, p.name, p.sku, p.created_at,
                 (SELECT group_concat(m.language, ',')
                    FROM (SELECT m.language FROM manuals m
                            JOIN languages l ON l.code = m.language
                           WHERE m.product_id = p.id
                           ORDER BY l.sort_order) m) AS languages
            FROM products p
           WHERE p.search_text LIKE ? ESCAPE '\'
           ORDER BY p.name COLLATE NOCASE, p.id",
    )
    .bind(pattern)
    .fetch_all(db)
    .await?)
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', r"\\")
        .replace('%', r"\%")
        .replace('_', r"\_")
}

pub async fn get(db: &SqlitePool, id: i64) -> anyhow::Result<Option<Product>> {
    Ok(sqlx::query_as(
        "SELECT id, name, sku, description, public_slug, default_language, created_at
           FROM products WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(db)
    .await?)
}

pub fn is_valid_slug(slug: &str) -> bool {
    slug.len() == SLUG_LEN && slug.bytes().all(|b| SLUG_ALPHABET.contains(&b))
}

pub async fn get_by_slug(db: &SqlitePool, slug: &str) -> anyhow::Result<Option<Product>> {
    Ok(sqlx::query_as(
        "SELECT id, name, sku, description, public_slug, default_language, created_at
           FROM products WHERE public_slug = ?",
    )
    .bind(slug)
    .fetch_optional(db)
    .await?)
}

pub async fn create(db: &SqlitePool, input: &ProductInput) -> Result<i64, SaveError> {
    // Сблъсък на slug е почти невъзможен, но ако стане, се пробва с нов.
    for _ in 0..5 {
        let result = sqlx::query(
            "INSERT INTO products (name, sku, description, default_language, search_text, public_slug)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&input.name)
        .bind(&input.sku)
        .bind(&input.description)
        .bind(&input.default_language)
        .bind(input.search_text())
        .bind(new_slug())
        .execute(db)
        .await;

        match result {
            Ok(done) => return Ok(done.last_insert_rowid()),
            Err(err) if is_unique_violation(&err, "products.public_slug") => continue,
            Err(err) => return Err(err.into()),
        }
    }
    Err(SaveError::Other(anyhow::anyhow!("не може да се генерира уникален slug")))
}

/// Връща false, ако продуктът не съществува.
pub async fn update(db: &SqlitePool, id: i64, input: &ProductInput) -> Result<bool, SaveError> {
    let done = sqlx::query(
        "UPDATE products
            SET name = ?, sku = ?, description = ?, default_language = ?, search_text = ?,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
          WHERE id = ?",
    )
    .bind(&input.name)
    .bind(&input.sku)
    .bind(&input.description)
    .bind(&input.default_language)
    .bind(input.search_text())
    .bind(id)
    .execute(db)
    .await?;
    Ok(done.rows_affected() > 0)
}

/// Изтрива продукта с упътванията му. Връща false, ако не съществува.
pub async fn delete(db: &SqlitePool, id: i64) -> anyhow::Result<bool> {
    let done = sqlx::query("DELETE FROM products WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    Ok(done.rows_affected() > 0)
}

fn new_slug() -> String {
    // Отхвърляне на стойности над кратното, за да няма изкривяване към първите символи.
    let limit = 256 - (256 % SLUG_ALPHABET.len());
    let mut slug = String::with_capacity(SLUG_LEN);
    while slug.len() < SLUG_LEN {
        let mut byte = [0u8; 1];
        OsRng.fill_bytes(&mut byte);
        if (byte[0] as usize) < limit {
            slug.push(SLUG_ALPHABET[byte[0] as usize % SLUG_ALPHABET.len()] as char);
        }
    }
    slug
}

#[cfg(test)]
mod tests {
    use super::*;

    fn langs() -> Vec<Language> {
        vec![Language {
            code: "bg".into(),
            name: "Български".into(),
        }]
    }

    #[test]
    fn slug_uses_only_unambiguous_characters() {
        for _ in 0..200 {
            let slug = new_slug();
            assert_eq!(slug.len(), SLUG_LEN);
            assert!(slug.bytes().all(|b| SLUG_ALPHABET.contains(&b)), "{slug}");
        }
    }

    #[test]
    fn validation_trims_and_requires_name() {
        let form = ProductForm {
            name: "   ".into(),
            default_language: "xx".into(),
            ..Default::default()
        };
        let errors = form.validate(&langs()).err().unwrap();
        assert!(errors.name.is_some());
        assert!(errors.default_language.is_some());

        let form = ProductForm {
            name: " Стелка Sport ".into(),
            sku: "  ".into(),
            default_language: "bg".into(),
            ..Default::default()
        };
        let input = form.validate(&langs()).ok().unwrap();
        assert_eq!(input.name, "Стелка Sport");
        assert_eq!(input.sku, None);
        assert_eq!(input.search_text(), "стелка sport ");
    }

    #[test]
    fn like_wildcards_are_escaped() {
        assert_eq!(escape_like(r"50%_a\b"), r"50\%\_a\\b");
    }

    #[test]
    fn dates_are_shown_day_first() {
        assert_eq!(display_date("2026-11-01T19:17:33Z"), "01.11.2026");
    }
}
