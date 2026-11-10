//! Защита на публичните адреси (`/m/`, `/f/`) от crawler-и и масово теглене.
//!
//! Линковете са случайни и не могат да се налучкат (55⁸ за `/m/`, 2¹²⁸ за `/f/`), така че
//! crawler може да тегли само линкове, които вече има. Тук се ограничава колко бързо:
//! - страници `/m/`: 60 в минута от IP. Човек, който сканира QR кодове, не стига до това;
//! - файлове `/f/`: 600 в минута. Повечето файлове ги сервира кешът на Cloudflare и не
//!   стигат до тук, затова основното ограничение за тях е правило в Cloudflare (DEPLOY.md);
//! - налучкване: 30 несъществуващи линка за 10 минути блокират IP-то за 30 минути.
//!   Нормален потребител никога не получава 404, защото отваря линк от QR код.
//!
//! Броячите са в паметта и се нулират при рестарт, както при ограничението на входа.

use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::Mutex,
    time::{Duration, Instant},
};

use axum::{
    extract::{ConnectInfo, Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

use super::{public::StatusTemplate, rate_limit::client_ip, AppState};

pub const WINDOW: Duration = Duration::from_secs(60);
pub const MAX_PAGES: u32 = 60;
pub const MAX_FILES: u32 = 600;
pub const MISS_WINDOW: Duration = Duration::from_secs(10 * 60);
pub const MAX_MISSES: u32 = 30;
pub const BAN: Duration = Duration::from_secs(30 * 60);
/// Над толкова записа изтеклите се изчистват, за да не расте паметта безкрайно.
const PRUNE_ABOVE: usize = 20_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Page,
    File,
    Miss,
}

impl Kind {
    fn limit(self) -> (u32, Duration) {
        match self {
            Kind::Page => (MAX_PAGES, WINDOW),
            Kind::File => (MAX_FILES, WINDOW),
            Kind::Miss => (MAX_MISSES, MISS_WINDOW),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Window {
    started: Instant,
    count: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    /// Отказ. `first` е true при първия отказ в прозореца, за да се логне веднъж.
    Block { wait: Duration, first: bool },
}

#[derive(Default)]
pub struct PublicGuard {
    counters: Mutex<HashMap<(IpAddr, Kind), Window>>,
    banned: Mutex<HashMap<IpAddr, Instant>>,
}

impl PublicGuard {
    /// Брои заявката и казва дали да мине.
    pub fn check(&self, ip: IpAddr, kind: Kind, now: Instant) -> Verdict {
        if let Some(until) = self.banned.lock().expect("guard lock").get(&ip) {
            if *until > now {
                return Verdict::Block { wait: *until - now, first: false };
            }
        }

        let (max, window_len) = kind.limit();
        let mut counters = self.counters.lock().expect("guard lock");
        let window = counter(&mut counters, ip, kind, window_len, now);
        if window.count >= max {
            let wait = window_len.saturating_sub(now.saturating_duration_since(window.started));
            let first = window.count == max;
            window.count = window.count.saturating_add(1);
            return Verdict::Block { wait: wait.max(Duration::from_secs(1)), first };
        }
        window.count += 1;
        Verdict::Allow
    }

    /// Отбелязва несъществуващ линк. Връща true, ако с това IP-то току-що е блокирано.
    pub fn record_miss(&self, ip: IpAddr, now: Instant) -> bool {
        let mut counters = self.counters.lock().expect("guard lock");
        let window = counter(&mut counters, ip, Kind::Miss, MISS_WINDOW, now);
        window.count += 1;
        if window.count < MAX_MISSES {
            return false;
        }
        counters.remove(&(ip, Kind::Miss));
        drop(counters);

        let mut banned = self.banned.lock().expect("guard lock");
        if banned.len() > PRUNE_ABOVE {
            banned.retain(|_, until| *until > now);
        }
        banned.insert(ip, now + BAN);
        true
    }
}

fn counter(
    counters: &mut HashMap<(IpAddr, Kind), Window>,
    ip: IpAddr,
    kind: Kind,
    window_len: Duration,
    now: Instant,
) -> &mut Window {
    if counters.len() > PRUNE_ABOVE {
        counters.retain(|(_, k), w| now.saturating_duration_since(w.started) < k.limit().1);
    }
    let window = counters.entry((ip, kind)).or_insert(Window { started: now, count: 0 });
    if now.saturating_duration_since(window.started) >= window_len {
        *window = Window { started: now, count: 0 };
    }
    window
}

/// Middleware за публичните адреси.
pub async fn guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let Some(ConnectInfo(peer)) = request.extensions().get::<ConnectInfo<SocketAddr>>().copied() else {
        return next.run(request).await;
    };
    let ip = client_ip(peer, request.headers(), state.config.trust_proxy);
    let kind = if request.uri().path().starts_with("/f/") { Kind::File } else { Kind::Page };
    let now = Instant::now();

    if let Verdict::Block { wait, first } = state.public_guard.check(ip, kind, now) {
        if first {
            tracing::warn!(%ip, ?kind, "твърде много заявки, временно спрян");
        }
        return too_many(wait);
    }

    let response = next.run(request).await;
    if response.status() == StatusCode::NOT_FOUND && state.public_guard.record_miss(ip, now) {
        tracing::warn!(%ip, minutes = BAN.as_secs() / 60, "блокиран: много несъществуващи линкове");
    }
    response
}

fn too_many(wait: Duration) -> Response {
    let page = askama::Template::render(&StatusTemplate { code: 429 }).unwrap_or_default();
    (
        StatusCode::TOO_MANY_REQUESTS,
        [
            (header::RETRY_AFTER, wait.as_secs().max(1).to_string()),
            (header::CONTENT_TYPE, "text/html; charset=utf-8".to_owned()),
            (header::CACHE_CONTROL, "no-store".to_owned()),
        ],
        page,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(last: u8) -> IpAddr {
        IpAddr::from([10, 0, 0, last])
    }

    #[test]
    fn pages_are_limited_per_ip_and_window() {
        let guard = PublicGuard::default();
        let start = Instant::now();
        for _ in 0..MAX_PAGES {
            assert_eq!(guard.check(ip(1), Kind::Page, start), Verdict::Allow);
        }
        let blocked = guard.check(ip(1), Kind::Page, start + Duration::from_secs(20));
        assert_eq!(blocked, Verdict::Block { wait: Duration::from_secs(40), first: true });
        // Логва се само първият отказ.
        assert!(matches!(guard.check(ip(1), Kind::Page, start), Verdict::Block { first: false, .. }));
        // Друг адрес и файловете на същия адрес не са засегнати.
        assert_eq!(guard.check(ip(2), Kind::Page, start), Verdict::Allow);
        assert_eq!(guard.check(ip(1), Kind::File, start), Verdict::Allow);
        // След прозореца отново може.
        assert_eq!(guard.check(ip(1), Kind::Page, start + WINDOW), Verdict::Allow);
    }

    #[test]
    fn guessing_links_bans_the_ip_for_everything() {
        let guard = PublicGuard::default();
        let now = Instant::now();
        for _ in 0..MAX_MISSES - 1 {
            assert!(!guard.record_miss(ip(1), now));
        }
        assert!(guard.record_miss(ip(1), now));
        assert!(matches!(guard.check(ip(1), Kind::Page, now), Verdict::Block { .. }));
        assert!(matches!(guard.check(ip(1), Kind::File, now), Verdict::Block { .. }));
        assert_eq!(guard.check(ip(2), Kind::Page, now), Verdict::Allow);
        assert_eq!(guard.check(ip(1), Kind::Page, now + BAN), Verdict::Allow);
    }

    #[test]
    fn misses_spread_over_time_do_not_ban() {
        let guard = PublicGuard::default();
        let start = Instant::now();
        for i in 0..MAX_MISSES * 3 {
            // Една грешка на всеки 30 s: под прага от 30 за 10 минути.
            assert!(!guard.record_miss(ip(1), start + Duration::from_secs(30 * u64::from(i))));
        }
    }
}
