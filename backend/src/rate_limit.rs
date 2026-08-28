//! Sliding-window rate limiter.
//! Default: 120 requests per minute per client IP (and per authenticated user).
//! Exceeding the limit returns HTTP 429 with a `Retry-After` header, which
//! helps mitigate DoS / brute-force / spam attacks.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct RateLimiter {
    requests: u32,
    window: Duration,
    map: Mutex<HashMap<String, VecDeque<Instant>>>,
    insert_count: Mutex<u64>,
}

impl RateLimiter {
    pub fn requests_per_minute(&self) -> u32 {
        self.requests
    }

    pub fn new(requests: u32, window_secs: u64) -> Self {
        Self {
            requests,
            window: Duration::from_secs(window_secs),
            map: Mutex::new(HashMap::new()),
            insert_count: Mutex::new(0),
        }
    }

    /// Returns Ok(()) if allowed, or Err(retry_after_secs) if blocked.
    pub fn check(&self, key: &str) -> Result<(), u64> {
        let now = Instant::now();
        let mut map = self.map.lock().expect("rate limiter lock poisoned");
        let queue = map.entry(key.to_string()).or_default();

        // Drop expired entries.
        while let Some(front) = queue.front() {
            if now.duration_since(*front) >= self.window {
                queue.pop_front();
            } else {
                break;
            }
        }

        if queue.len() >= self.requests as usize {
            let oldest = queue.front().copied().unwrap_or(now);
            let remaining = self.window.saturating_sub(now.duration_since(oldest));
            return Err(remaining.as_secs().max(1));
        }

        queue.push_back(now);

        // Opportunistic cleanup of stale keys.
        let mut counter = self.insert_count.lock().expect("counter lock poisoned");
        *counter += 1;
        if *counter % 10_000 == 0 {
            map.retain(|_, q| q.back().is_some_and(|t| now.duration_since(*t) < self.window));
        }
        Ok(())
    }
}

/// Determine the client IP, trusting `X-Forwarded-For` only when configured
/// (never trust it directly from the internet by default).
pub fn client_ip(cfg: &crate::config::Config, headers: &axum::http::HeaderMap) -> String {
    if cfg.trust_proxy {
        if let Some(v) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
            if let Some(first) = v.split(',').next() {
                let ip = first.trim();
                if !ip.is_empty() {
                    return ip.to_string();
                }
            }
        }
        if let Some(v) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
            if !v.trim().is_empty() {
                return v.trim().to_string();
            }
        }
    }
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}
