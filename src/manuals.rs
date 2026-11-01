//! Упътванията (едно на език за продукт) и файловете в тях.
//! Упътване съществува, докато има поне един файл: създава се при първото качване
//! и се изтрива заедно с последния файл.

use sqlx::SqlitePool;

use crate::storage::FileKind;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct FileRow {
    pub id: i64,
    pub original_name: String,
    pub stored_name: String,
    pub mime: String,
    pub size_bytes: i64,
    /// Намаленото копие на снимка, ако има такова (`images::display_copy`).
    pub display_name: Option<String>,
    pub display_size: Option<i64>,
}

impl FileRow {
    pub fn is_pdf(&self) -> bool {
        self.mime == FileKind::Pdf.mime()
    }

    /// Името на файла, който се показва на страниците: копието, ако има, иначе оригиналът.
    pub fn shown_name(&self) -> &str {
        self.display_name.as_deref().unwrap_or(&self.stored_name)
    }

    pub fn size_display(&self) -> String {
        format_size(self.size_bytes)
    }

    pub fn display_size_text(&self) -> Option<String> {
        self.display_size.map(format_size)
    }
}

fn format_size(bytes: i64) -> String {
    let size = bytes as f64;
    if size >= 1024.0 * 1024.0 {
        format!("{:.1} MB", size / 1024.0 / 1024.0)
    } else {
        format!("{} KB", (size / 1024.0).ceil() as i64)
    }
}

/// Броят файлове за всеки език, на който продуктът има упътване.
pub async fn file_counts(db: &SqlitePool, product_id: i64) -> anyhow::Result<Vec<(String, i64)>> {
    Ok(sqlx::query_as(
        "SELECT m.language, COUNT(f.id) FROM manuals m
           JOIN files f ON f.manual_id = m.id
          WHERE m.product_id = ?
          GROUP BY m.language",
    )
    .bind(product_id)
    .fetch_all(db)
    .await?)
}

/// Езиците, на които продуктът има упътване, в реда от таблица `languages`.
pub async fn languages_with_files(db: &SqlitePool, product_id: i64) -> anyhow::Result<Vec<String>> {
    Ok(sqlx::query_scalar(
        "SELECT m.language FROM manuals m JOIN languages l ON l.code = m.language
          WHERE m.product_id = ? AND EXISTS (SELECT 1 FROM files f WHERE f.manual_id = m.id)
          ORDER BY l.sort_order",
    )
    .bind(product_id)
    .fetch_all(db)
    .await?)
}

pub async fn files(db: &SqlitePool, product_id: i64, language: &str) -> anyhow::Result<Vec<FileRow>> {
    Ok(sqlx::query_as(
        "SELECT f.id, f.original_name, f.stored_name, f.mime, f.size_bytes,
                f.display_name, f.display_size
           FROM files f JOIN manuals m ON m.id = f.manual_id
          WHERE m.product_id = ? AND m.language = ?
          ORDER BY f.sort_order, f.id",
    )
    .bind(product_id)
    .bind(language)
    .fetch_all(db)
    .await?)
}

pub struct NewFile<'a> {
    pub original_name: &'a str,
    pub stored_name: &'a str,
    pub kind: FileKind,
    pub size_bytes: i64,
    pub display_name: Option<&'a str>,
    pub display_size: Option<i64>,
}

/// Добавя файл накрая на упътването, като създава упътването, ако го няма.
pub async fn add_file(
    db: &SqlitePool,
    product_id: i64,
    language: &str,
    file: &NewFile<'_>,
) -> anyhow::Result<()> {
    let mut tx = db.begin().await?;
    sqlx::query("INSERT INTO manuals (product_id, language) VALUES (?, ?) ON CONFLICT DO NOTHING")
        .bind(product_id)
        .bind(language)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO files (manual_id, original_name, stored_name, mime, size_bytes,
                            display_name, display_size, sort_order)
         SELECT m.id, ?, ?, ?, ?, ?, ?,
                COALESCE((SELECT MAX(sort_order) + 1 FROM files WHERE manual_id = m.id), 0)
           FROM manuals m WHERE m.product_id = ? AND m.language = ?",
    )
    .bind(file.original_name)
    .bind(file.stored_name)
    .bind(file.kind.mime())
    .bind(file.size_bytes)
    .bind(file.display_name)
    .bind(file.display_size)
    .bind(product_id)
    .bind(language)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Езикът на упътването, в което е файлът, ако файлът е на този продукт.
pub async fn file_language(
    db: &SqlitePool,
    product_id: i64,
    file_id: i64,
) -> anyhow::Result<Option<String>> {
    Ok(sqlx::query_scalar(
        "SELECT m.language FROM files f JOIN manuals m ON m.id = f.manual_id
          WHERE f.id = ? AND m.product_id = ?",
    )
    .bind(file_id)
    .bind(product_id)
    .fetch_optional(db)
    .await?)
}

/// Изтрива файла от базата и връща имената му на диска (оригинал и копие).
/// Изтрива и празното упътване.
pub async fn delete_file(db: &SqlitePool, file_id: i64) -> anyhow::Result<Vec<String>> {
    let mut tx = db.begin().await?;
    let row: Option<(String, Option<String>, i64)> = sqlx::query_as(
        "DELETE FROM files WHERE id = ? RETURNING stored_name, display_name, manual_id",
    )
    .bind(file_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((stored_name, display_name, manual_id)) = row else {
        return Ok(Vec::new());
    };
    sqlx::query("DELETE FROM manuals WHERE id = ? AND NOT EXISTS (SELECT 1 FROM files WHERE manual_id = ?)")
        .bind(manual_id)
        .bind(manual_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok([Some(stored_name), display_name].into_iter().flatten().collect())
}

/// Разменя файла със съседния нагоре или надолу. На края на списъка не прави нищо.
pub async fn move_file(db: &SqlitePool, file_id: i64, up: bool) -> anyhow::Result<()> {
    let mut tx = db.begin().await?;
    // Подредбата се нормализира първо, за да няма еднакви sort_order и дупки.
    let ids: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM files
          WHERE manual_id = (SELECT manual_id FROM files WHERE id = ?)
          ORDER BY sort_order, id",
    )
    .bind(file_id)
    .fetch_all(&mut *tx)
    .await?;

    let mut ids = ids;
    if let Some(pos) = ids.iter().position(|&id| id == file_id) {
        let target = if up { pos.checked_sub(1) } else { Some(pos + 1) };
        if let Some(target) = target.filter(|&t| t < ids.len()) {
            ids.swap(pos, target);
        }
    }
    for (order, id) in ids.iter().enumerate() {
        sqlx::query("UPDATE files SET sort_order = ? WHERE id = ?")
            .bind(order as i64)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Имената на диска на всички файлове на продукта, с копията (преди изтриването му).
pub async fn stored_names_for_product(db: &SqlitePool, product_id: i64) -> anyhow::Result<Vec<String>> {
    Ok(sqlx::query_scalar(
        "SELECT name FROM (
             SELECT f.stored_name AS name, m.product_id FROM files f JOIN manuals m ON m.id = f.manual_id
             UNION ALL
             SELECT f.display_name, m.product_id FROM files f JOIN manuals m ON m.id = f.manual_id
              WHERE f.display_name IS NOT NULL)
          WHERE product_id = ?",
    )
    .bind(product_id)
    .fetch_all(db)
    .await?)
}

/// Файлът, на който е това име на диска: оригиналът или копието му.
pub async fn find_by_disk_name(db: &SqlitePool, name: &str) -> anyhow::Result<Option<FileRow>> {
    Ok(sqlx::query_as(
        "SELECT id, original_name, stored_name, mime, size_bytes, display_name, display_size
           FROM files WHERE stored_name = ? OR display_name = ?",
    )
    .bind(name)
    .bind(name)
    .fetch_optional(db)
    .await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(size_bytes: i64) -> FileRow {
        FileRow {
            id: 1,
            original_name: String::new(),
            stored_name: String::new(),
            mime: "application/pdf".into(),
            size_bytes,
            display_name: None,
            display_size: None,
        }
    }

    #[test]
    fn sizes_are_human_readable() {
        assert_eq!(file(500).size_display(), "1 KB");
        assert_eq!(file(150 * 1024).size_display(), "150 KB");
        assert_eq!(file(5 * 1024 * 1024 + 300_000).size_display(), "5.3 MB");
    }
}
