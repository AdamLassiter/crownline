use std::{
    collections::BTreeMap,
    env,
    net::IpAddr,
    time::{Duration, Instant},
};

use tracing::warn;

#[derive(Debug, Clone, Copy)]
pub struct ServerLimits {
    pub max_http_body_bytes: usize,
    pub max_rooms: usize,
    pub create_per_ip_per_minute: u32,
    pub join_per_ip_per_minute: u32,
    pub operations_per_room_per_minute: u32,
    pub max_connections_total: usize,
    pub max_connections_per_ip: usize,
    pub pregame_idle_timeout: Duration,
}

impl Default for ServerLimits {
    fn default() -> Self {
        Self {
            max_http_body_bytes: crownline_protocol::MAX_HTTP_REQUEST_BYTES,
            max_rooms: 1_000,
            create_per_ip_per_minute: 10,
            join_per_ip_per_minute: 30,
            operations_per_room_per_minute: 120,
            max_connections_total: 2_000,
            max_connections_per_ip: 8,
            pregame_idle_timeout: Duration::from_mins(30),
        }
    }
}

impl ServerLimits {
    /// Loads optional deployment overrides while retaining safe defaults.
    ///
    /// # Errors
    ///
    /// Returns a field-specific message when an override is not a positive integer.
    pub fn from_env() -> Result<Self, String> {
        let mut limits = Self::default();
        override_value("CROWNLINE_MAX_HTTP_BYTES", &mut limits.max_http_body_bytes)?;
        override_value("CROWNLINE_MAX_ROOMS", &mut limits.max_rooms)?;
        override_value(
            "CROWNLINE_CREATE_PER_MINUTE",
            &mut limits.create_per_ip_per_minute,
        )?;
        override_value(
            "CROWNLINE_JOIN_PER_MINUTE",
            &mut limits.join_per_ip_per_minute,
        )?;
        override_value(
            "CROWNLINE_ROOM_OPERATIONS_PER_MINUTE",
            &mut limits.operations_per_room_per_minute,
        )?;
        override_value(
            "CROWNLINE_MAX_CONNECTIONS",
            &mut limits.max_connections_total,
        )?;
        override_value(
            "CROWNLINE_MAX_CONNECTIONS_PER_IP",
            &mut limits.max_connections_per_ip,
        )?;
        if let Some(seconds) = read_override::<u64>("CROWNLINE_PREGAME_IDLE_SECONDS")? {
            limits.pregame_idle_timeout = Duration::from_secs(seconds);
        }
        Ok(limits)
    }
}

fn override_value<T>(name: &str, target: &mut T) -> Result<(), String>
where
    T: std::str::FromStr + Default + PartialEq,
{
    if let Some(value) = read_override(name)? {
        *target = value;
    }
    Ok(())
}

fn read_override<T>(name: &str) -> Result<Option<T>, String>
where
    T: std::str::FromStr + Default + PartialEq,
{
    let Some(raw) = env::var_os(name) else {
        return Ok(None);
    };
    let raw = raw
        .into_string()
        .map_err(|_| format!("{name} must be valid UTF-8"))?;
    let parsed = raw
        .parse::<T>()
        .map_err(|_| format!("{name} must be a positive integer"))?;
    if parsed == T::default() {
        return Err(format!("{name} must be a positive integer"));
    }
    Ok(Some(parsed))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LimitKind {
    Create,
    Join,
    RoomOperation,
}

#[derive(Debug, Clone, Copy)]
struct Window {
    started: Instant,
    count: u32,
}

#[derive(Debug, Default)]
pub struct RequestLimiter {
    windows: BTreeMap<(LimitKind, String), Window>,
}

impl RequestLimiter {
    pub fn check(
        &mut self,
        kind: LimitKind,
        scope: impl Into<String>,
        maximum: u32,
        now: Instant,
    ) -> bool {
        let scope = scope.into();
        let window = self.windows.entry((kind, scope.clone())).or_insert(Window {
            started: now,
            count: 0,
        });
        if now.duration_since(window.started) >= Duration::from_mins(1) {
            *window = Window {
                started: now,
                count: 0,
            };
        }
        if window.count >= maximum {
            warn!(?kind, %scope, maximum, "request limit reached");
            return false;
        }
        window.count = window.count.saturating_add(1);
        true
    }

    pub fn discard_expired(&mut self, now: Instant) {
        self.windows
            .retain(|_, window| now.duration_since(window.started) < Duration::from_mins(1));
    }

    pub fn tracked_scopes(&self) -> usize {
        self.windows.len()
    }
}

#[derive(Debug, Default)]
pub struct ConnectionRegistry {
    total: usize,
    per_ip: BTreeMap<IpAddr, usize>,
}

impl ConnectionRegistry {
    pub fn try_open(&mut self, ip: IpAddr, limits: &ServerLimits) -> bool {
        let for_ip = self.per_ip.get(&ip).copied().unwrap_or_default();
        if self.total >= limits.max_connections_total || for_ip >= limits.max_connections_per_ip {
            warn!(%ip, total = self.total, for_ip, "connection limit reached");
            return false;
        }
        self.total += 1;
        self.per_ip.insert(ip, for_ip + 1);
        true
    }

    pub fn close(&mut self, ip: IpAddr) {
        let Some(for_ip) = self.per_ip.get_mut(&ip) else {
            return;
        };
        *for_ip = for_ip.saturating_sub(1);
        self.total = self.total.saturating_sub(1);
        if *for_ip == 0 {
            self.per_ip.remove(&ip);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_windows_limit_then_expire_without_unbounded_scope_growth() {
        let now = Instant::now();
        let mut limiter = RequestLimiter::default();
        assert!(limiter.check(LimitKind::Create, "127.0.0.1", 2, now));
        assert!(limiter.check(LimitKind::Create, "127.0.0.1", 2, now));
        assert!(!limiter.check(LimitKind::Create, "127.0.0.1", 2, now));
        limiter.discard_expired(now + Duration::from_mins(1));
        assert_eq!(limiter.tracked_scopes(), 0);
    }

    #[test]
    fn connections_are_bounded_per_ip_and_globally_and_release_capacity() {
        let limits = ServerLimits {
            max_connections_total: 2,
            max_connections_per_ip: 1,
            ..ServerLimits::default()
        };
        let first = IpAddr::from([127, 0, 0, 1]);
        let second = IpAddr::from([127, 0, 0, 2]);
        let mut registry = ConnectionRegistry::default();
        assert!(registry.try_open(first, &limits));
        assert!(!registry.try_open(first, &limits));
        assert!(registry.try_open(second, &limits));
        registry.close(first);
        assert!(registry.try_open(first, &limits));
    }
}
