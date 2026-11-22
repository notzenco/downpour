use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Token bucket shared by every connection it throttles. Callers take bytes
/// first and then sleep off any debt, so concurrent connections split the
/// budget fairly without a central scheduler.
#[derive(Debug)]
pub struct RateLimiter {
    limit: AtomicU64,
    bucket: Mutex<Bucket>,
}

#[derive(Debug)]
struct Bucket {
    tokens: f64,
    last: Instant,
}

impl RateLimiter {
    pub fn new(bytes_per_sec: u64) -> Self {
        RateLimiter {
            limit: AtomicU64::new(bytes_per_sec),
            bucket: Mutex::new(Bucket {
                tokens: 0.0,
                last: Instant::now(),
            }),
        }
    }

    pub fn set_limit(&self, bytes_per_sec: u64) {
        self.limit.store(bytes_per_sec, Ordering::Relaxed);
        let mut b = self.bucket.lock().unwrap();
        b.tokens = 0.0;
        b.last = Instant::now();
    }

    /// How long the caller must wait before using `bytes`.
    pub fn reserve(&self, bytes: u64) -> Duration {
        let limit = self.limit.load(Ordering::Relaxed);
        if limit == 0 {
            return Duration::ZERO;
        }
        let rate = limit as f64;
        let mut b = self.bucket.lock().unwrap();
        let now = Instant::now();
        // Allow at most a quarter-second burst so the limit holds over short windows.
        b.tokens = (b.tokens + now.duration_since(b.last).as_secs_f64() * rate).min(rate / 4.0);
        b.last = now;
        b.tokens -= bytes as f64;
        if b.tokens >= 0.0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f64(-b.tokens / rate)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlimited_never_waits() {
        let l = RateLimiter::new(0);
        assert_eq!(l.reserve(u64::MAX / 2), Duration::ZERO);
    }

    #[test]
    fn debt_converts_to_wait_at_the_configured_rate() {
        let l = RateLimiter::new(1000);
        let wait = l.reserve(2000);
        assert!(
            wait >= Duration::from_millis(1900) && wait <= Duration::from_millis(2100),
            "{wait:?}"
        );
    }
}
