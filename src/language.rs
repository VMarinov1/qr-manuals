//! Избор на език за публичната страница.

/// Езиците от `Accept-Language` (напр. `de-AT,de;q=0.9,en;q=0.5`) като основни
/// кодове с малки букви, подредени по предпочитание. Езици с q=0 се пропускат.
pub fn preferred(accept_language: &str) -> Vec<String> {
    let mut entries: Vec<(String, f32)> = accept_language
        .split(',')
        .filter_map(|part| {
            let mut pieces = part.trim().split(';');
            let tag = pieces.next()?.trim();
            let primary = tag.split(['-', '_']).next()?.trim().to_ascii_lowercase();
            if primary.is_empty() || primary == "*" {
                return None;
            }
            let quality = pieces
                .find_map(|p| p.trim().strip_prefix("q="))
                .map_or(Some(1.0), |q| q.trim().parse::<f32>().ok())?;
            (quality > 0.0).then_some((primary, quality))
        })
        .collect();
    // Стабилно сортиране: при равен q остава редът от заглавието.
    entries.sort_by(|a, b| b.1.total_cmp(&a.1));

    let mut codes: Vec<String> = Vec::new();
    for (code, _) in entries {
        if !codes.contains(&code) {
            codes.push(code);
        }
    }
    codes
}

/// Езикът на телефона, ако има упътване на него; иначе езикът по подразбиране
/// на продукта; иначе първият наличен. None, ако продуктът няма упътвания.
pub fn choose<'a>(
    accept_language: Option<&str>,
    default: &str,
    available: &'a [String],
) -> Option<&'a str> {
    let wanted = accept_language.map(preferred).unwrap_or_default();
    wanted
        .iter()
        .find_map(|code| available.iter().find(|a| *a == code))
        .or_else(|| available.iter().find(|a| *a == default))
        .or_else(|| available.first())
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn langs(codes: &[&str]) -> Vec<String> {
        codes.iter().map(|c| c.to_string()).collect()
    }

    #[test]
    fn parses_accept_language() {
        assert_eq!(preferred("de-AT,de;q=0.9,en;q=0.5"), ["de", "en"]);
        assert_eq!(preferred("en;q=0.3, ro, bg;q=0.8"), ["ro", "bg", "en"]);
        assert_eq!(preferred("fr;q=0, en"), ["en"]);
        assert_eq!(preferred("*, bg-BG"), ["bg"]);
        assert_eq!(preferred("EN-us"), ["en"]);
        assert_eq!(preferred("en;q=abc, bg"), ["bg"]);
        assert!(preferred("").is_empty());
    }

    #[test]
    fn chooses_phone_language_then_default_then_first() {
        let available = langs(&["bg", "en", "de"]);
        assert_eq!(choose(Some("de-DE,de;q=0.9"), "bg", &available), Some("de"));
        assert_eq!(choose(Some("fr-FR,en;q=0.7"), "bg", &available), Some("en"));
        assert_eq!(choose(Some("fr-FR"), "bg", &available), Some("bg"));
        assert_eq!(choose(None, "en", &available), Some("en"));
        assert_eq!(choose(Some("fr"), "ro", &langs(&["de", "en"])), Some("de"));
        assert_eq!(choose(Some("bg"), "bg", &[]), None);
    }
}
