//! Ограничение на неуспешните опити за вход, в паметта на процеса.
//!
//! Две граници в един и същ 15-минутен прозорец:
//! - от един IP адрес: 10 грешни опита, срещу налучкване от една машина;
//! - за един email: 30 грешни опита от всякакви адреси, срещу разпределени атаки.
//!   Границата е по-висока, за да е трудно някой нарочно да заключи чужд акаунт.
//!
//! Блокиран опит не стига до проверката на паролата, така че не товари и процесора.
//! При рестарт броячите се нулират, което е приемливо за един контейнер.

use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::Mutex,
    time::{Duration, Instant},
};

use axum::http::HeaderMap;

pub const WINDOW: Duration = Duration::from_secs(15 * 60);
pub const MAX_PER_IP: u32 = 10;
pub const MAX_PER_EMAIL: u32 = 30;
/// Над толкова записа старите се изчистват, за да не расте паметта безкрайно.
const PRUNE_ABOVE: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Key {
    Ip(IpAddr),
    Email(String),
}

#[derive(Debug, Clone, Copy)]
struct Window {
    started: Instant,
    failures: u32,
}

#[derive(Default)]
pub struct LoginLimiter {
    windows: Mutex<HashMap<Key, Window>>,
}

impl LoginLimiter {
    /// Ако опитът трябва да се откаже, връща след колко време може да се опита пак.
    pub fn blocked(&self, ip: IpAddr, email: &str, now: Instant) -> Option<Duration> {
        let windows = self.windows.lock().expect("limiter lock");
        [(Key::Ip(ip), MAX_PER_IP), (email_key(email), MAX_PER_EMAIL)]
            .into_iter()
            .filter_map(|(key, max)| {
                let window = windows.get(&key)?;
                let elapsed = now.saturating_duration_since(window.started);
                (elapsed < WINDOW && window.failures >= max).then(|| WINDOW - elapsed)
            })
            .max()
    }

    pub fn record_failure(&self, ip: IpAddr, email: &str, now: Instant) {
        let mut windows = self.windows.lock().expect("limiter lock");
        if windows.len() > PRUNE_ABOVE {
            windows.retain(|_, w| now.saturating_duration_since(w.started) < WINDOW);
        }
        for key in [Key::Ip(ip), email_key(email)] {
            let window = windows.entry(key).or_insert(Window {
                started: now,
                failures: 0,
            });
            if now.saturating_duration_since(window.started) >= WINDOW {
                *window = Window {
                    started: now,
                    failures: 0,
                };
            }
            window.failures += 1;
        }
    }

    /// Успешен вход нулира броячите на адреса и на акаунта.
    pub fn record_success(&self, ip: IpAddr, email: &str) {
        let mut windows = self.windows.lock().expect("limiter lock");
        windows.remove(&Key::Ip(ip));
        windows.remove(&email_key(email));
    }
}

fn email_key(email: &str) -> Key {
    Key::Email(email.trim().to_lowercase())
}

/// Адресът на клиента. Зад reverse proxy всички връзки идват от proxy-то, затова
/// при `trust_proxy` се взима последният адрес в `X-Forwarded-For` (добавен от
/// нашия proxy). По-ранните са подадени от клиента и могат да са фалшиви.
pub fn client_ip(peer: SocketAddr, headers: &HeaderMap, trust_proxy: bool) -> IpAddr {
    if trust_proxy {
        let forwarded = headers
            .get_all("x-forwarded-for")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(','))
            .filter_map(|ip| ip.trim().parse::<IpAddr>().ok())
            .last();
        if let Some(ip) = forwarded {
            return ip;
        }
    }
    peer.ip()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(last: u8) -> IpAddr {
        IpAddr::from([10, 0, 0, last])
    }

    #[test]
    fn blocks_ip_after_max_failures_until_window_ends() {
        let limiter = LoginLimiter::default();
        let start = Instant::now();
        for i in 0..MAX_PER_IP {
            assert_eq!(limiter.blocked(ip(1), &format!("u{i}@x.bg"), start), None);
            limiter.record_failure(ip(1), &format!("u{i}@x.bg"), start);
        }
        // Блокиран е адресът, независимо от email-а.
        let wait = limiter.blocked(ip(1), "someone@x.bg", start + Duration::from_secs(60));
        assert_eq!(wait, Some(WINDOW - Duration::from_secs(60)));
        // Друг адрес не е засегнат.
        assert_eq!(limiter.blocked(ip(2), "someone@x.bg", start), None);
        // След прозореца отново може.
        assert_eq!(limiter.blocked(ip(1), "someone@x.bg", start + WINDOW), None);
    }

    #[test]
    fn blocks_email_across_many_ips() {
        let limiter = LoginLimiter::default();
        let now = Instant::now();
        for i in 0..MAX_PER_EMAIL {
            limiter.record_failure(ip(i as u8), " Admin@X.bg ", now);
        }
        assert!(limiter.blocked(ip(200), "admin@x.bg", now).is_some());
        assert!(limiter.blocked(ip(200), "other@x.bg", now).is_none());
    }

    #[test]
    fn success_resets_counters() {
        let limiter = LoginLimiter::default();
        let now = Instant::now();
        for _ in 0..MAX_PER_IP - 1 {
            limiter.record_failure(ip(1), "a@x.bg", now);
        }
        limiter.record_success(ip(1), "a@x.bg");
        limiter.record_failure(ip(1), "a@x.bg", now);
        assert_eq!(limiter.blocked(ip(1), "a@x.bg", now), None);
    }

    #[test]
    fn client_ip_uses_last_forwarded_address_only_behind_proxy() {
        let peer: SocketAddr = "172.17.0.1:5000".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "6.6.6.6, 203.0.113.7".parse().unwrap());

        assert_eq!(client_ip(peer, &headers, false), peer.ip());
        assert_eq!(
            client_ip(peer, &headers, true),
            "203.0.113.7".parse::<IpAddr>().unwrap()
        );
        assert_eq!(client_ip(peer, &HeaderMap::new(), true), peer.ip());

        headers.insert("x-forwarded-for", "garbage".parse().unwrap());
        assert_eq!(client_ip(peer, &headers, true), peer.ip());
    }

    #[test]
    fn window_restarts_after_expiry() {
        let limiter = LoginLimiter::default();
        let start = Instant::now();
        for _ in 0..MAX_PER_IP - 1 {
            limiter.record_failure(ip(1), "a@x.bg", start);
        }
        // Един опит след края на прозореца започва нов прозорец от 1.
        limiter.record_failure(ip(1), "a@x.bg", start + WINDOW);
        assert_eq!(limiter.blocked(ip(1), "a@x.bg", start + WINDOW), None);
    }
}
