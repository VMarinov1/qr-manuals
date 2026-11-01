use std::path::{Path, PathBuf};

use argon2::password_hash::rand_core::{OsRng, RngCore};

/// Позволените видове файлове. Определят се по съдържанието, не по разширението.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Jpeg,
    Png,
    Webp,
    Pdf,
}

impl FileKind {
    /// Колко байта от началото на файла са нужни на `detect`.
    pub const HEAD_LEN: usize = 12;

    pub fn detect(head: &[u8]) -> Option<Self> {
        if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
            Some(Self::Jpeg)
        } else if head.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
            Some(Self::Png)
        } else if head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"WEBP" {
            Some(Self::Webp)
        } else if head.starts_with(b"%PDF-") {
            Some(Self::Pdf)
        } else {
            None
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
            Self::Webp => "image/webp",
            Self::Pdf => "application/pdf",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::Webp => "webp",
            Self::Pdf => "pdf",
        }
    }
}

pub const TEMP_PREFIX: &str = ".upload-";

pub fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    OsRng.fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

/// Случайно име, под което файлът се пази и сервира.
pub fn new_stored_name(kind: FileKind) -> String {
    format!("{}.{}", random_hex(16), kind.extension())
}

/// Пропуска всичко, което не изглежда като име от `new_stored_name`, за да не може
/// през URL да се стигне до друг път на диска.
pub fn is_valid_stored_name(name: &str) -> bool {
    match name.split_once('.') {
        Some((stem, ext)) => {
            stem.len() == 32
                && stem.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                && matches!(ext, "jpg" | "png" | "webp" | "pdf")
        }
        None => false,
    }
}

pub fn path_for(dir: &Path, stored_name: &str) -> PathBuf {
    dir.join(stored_name)
}

/// Изтрива файловете от диска. Липсващ файл не е грешка.
pub async fn remove_files(dir: &Path, stored_names: &[String]) {
    for name in stored_names {
        if let Err(err) = tokio::fs::remove_file(path_for(dir, name)).await {
            if err.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(file = %name, "не може да се изтрие: {err}");
            }
        }
    }
}

/// Чисти недовършени качвания, останали от спиране по време на качване.
pub async fn remove_temp_files(dir: &Path) -> std::io::Result<()> {
    let mut entries = tokio::fs::read_dir(dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        if entry.file_name().to_string_lossy().starts_with(TEMP_PREFIX) {
            tokio::fs::remove_file(entry.path()).await.ok();
        }
    }
    Ok(())
}

/// Името, което браузърът е изпратил, почистено за показване.
/// Някои браузъри пращат пълния път, затова се взима само последната част.
pub fn clean_original_name(raw: &str) -> String {
    let name = raw.rsplit(['/', '\\']).next().unwrap_or_default();
    name.chars()
        .filter(|c| !c.is_control())
        .take(200)
        .collect::<String>()
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_by_content() {
        assert_eq!(FileKind::detect(&[0xFF, 0xD8, 0xFF, 0xE0]), Some(FileKind::Jpeg));
        assert_eq!(
            FileKind::detect(b"\x89PNG\r\n\x1a\n\0\0\0\x0d"),
            Some(FileKind::Png)
        );
        assert_eq!(FileKind::detect(b"RIFF\x24\0\0\0WEBPVP8 "), Some(FileKind::Webp));
        assert_eq!(FileKind::detect(b"%PDF-1.7\n"), Some(FileKind::Pdf));
        assert_eq!(FileKind::detect(b"RIFF\x24\0\0\0WAVEfmt "), None);
        assert_eq!(FileKind::detect(b"<html><script>"), None);
        assert_eq!(FileKind::detect(b"PK\x03\x04"), None);
        assert_eq!(FileKind::detect(b""), None);
    }

    #[test]
    fn stored_names_are_validated() {
        let name = new_stored_name(FileKind::Pdf);
        assert!(is_valid_stored_name(&name), "{name}");
        assert!(!is_valid_stored_name("../app.db"));
        assert!(!is_valid_stored_name("0123456789abcdef0123456789abcdef.exe"));
        assert!(!is_valid_stored_name("0123456789ABCDEF0123456789abcdef.pdf"));
        assert!(!is_valid_stored_name("abc.pdf"));
    }

    #[test]
    fn original_names_are_cleaned() {
        assert_eq!(clean_original_name(r"C:\Users\x\Упътване.pdf"), "Упътване.pdf");
        assert_eq!(clean_original_name("a/b/снимка 1.jpg"), "снимка 1.jpg");
        assert_eq!(clean_original_name("  bad\nname.png "), "badname.png");
        assert_eq!(clean_original_name(""), "");
    }
}
