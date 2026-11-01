//! Съобщение, което оцелява един redirect (POST -> redirect -> GET).

use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use percent_encoding::{percent_decode_str, utf8_percent_encode, NON_ALPHANUMERIC};

const COOKIE: &str = "qrm_flash";
/// Бисквитката е до ~4 KB, а кирилицата се кодира 6 байта на буква.
const MAX_LINES: usize = 6;
const MAX_LINE_CHARS: usize = 90;

pub struct Flash {
    pub error: bool,
    pub lines: Vec<String>,
}

impl Flash {
    pub fn ok(line: impl Into<String>) -> Self {
        Self {
            error: false,
            lines: vec![line.into()],
        }
    }

    pub fn set(self, jar: CookieJar) -> CookieJar {
        let mut lines: Vec<String> = self
            .lines
            .iter()
            .take(MAX_LINES)
            .map(|line| line.chars().take(MAX_LINE_CHARS).collect())
            .collect();
        if self.lines.len() > MAX_LINES {
            lines.push(format!("… и още {}", self.lines.len() - MAX_LINES));
        }
        let raw = format!("{}{}", if self.error { 'e' } else { 'o' }, lines.join("\n"));
        jar.add(
            Cookie::build((COOKIE, utf8_percent_encode(&raw, NON_ALPHANUMERIC).to_string()))
                .path("/")
                .http_only(true)
                .same_site(SameSite::Lax),
        )
    }

    /// Чете съобщението и го изтрива, за да не се покаже втори път.
    pub fn take(jar: CookieJar) -> (CookieJar, Option<Flash>) {
        let Some(cookie) = jar.get(COOKIE) else {
            return (jar, None);
        };
        let flash = percent_decode_str(cookie.value())
            .decode_utf8()
            .ok()
            .and_then(|raw| {
                let mut chars = raw.chars();
                let error = match chars.next()? {
                    'e' => true,
                    'o' => false,
                    _ => return None,
                };
                let lines = chars.as_str().lines().map(str::to_owned).collect();
                Some(Flash { error, lines })
            });
        (jar.remove(Cookie::build(COOKIE).path("/")), flash)
    }
}
