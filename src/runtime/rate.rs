//! B9 rate limiting (07): a per-session token bucket over **control-class** calls, configured
//! per scope (`limits.rate`). `pace` (default) parks a call until a token exists — the manager's
//! dispatch now runs on the blocking pool, so the wait costs a worker, not the reactor — and
//! `reject` answers `rate_limited` with a retry hint. Both modes are bounded: a call whose wait
//! exceeds `max_wait_ms`, or one arriving with more than `max_queued` already parked, is refused
//! rather than allowed to accumulate an unbounded parked army.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::config::RateLimit;
use crate::ipc::ErrorEnvelope;

struct Bucket {
    tokens: f64,
    last: Instant,
}

impl Bucket {
    fn new(burst: u32) -> Self {
        Self {
            tokens: burst as f64,
            last: Instant::now(),
        }
    }

    fn refill(&mut self, rate: &RateLimit) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last).as_secs_f64();
        self.tokens = (self.tokens + elapsed * rate.per_sec as f64).min(rate.burst as f64);
        self.last = now;
    }

    /// Seconds until one token exists (0 when one is available now).
    fn wait(&self, rate: &RateLimit) -> f64 {
        if self.tokens >= 1.0 {
            0.0
        } else {
            (1.0 - self.tokens) / rate.per_sec as f64
        }
    }

    fn take(&mut self) -> bool {
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

struct SessionRate {
    /// Arrival-order gate: one caller at a time computes/waits its slot.
    gate: Mutex<()>,
    bucket: Mutex<Bucket>,
    queued: AtomicUsize,
}

#[derive(Default)]
pub struct RateLimiter {
    sessions: Mutex<HashMap<String, Arc<SessionRate>>>,
}

impl RateLimiter {
    fn entry(&self, key: &str, burst: u32) -> Arc<SessionRate> {
        let mut sessions = self.sessions.lock().expect("rate lock");
        sessions
            .entry(key.to_string())
            .or_insert_with(|| {
                Arc::new(SessionRate {
                    gate: Mutex::new(()),
                    bucket: Mutex::new(Bucket::new(burst)),
                    queued: AtomicUsize::new(0),
                })
            })
            .clone()
    }

    /// Consume one token for a control-class call, or refuse with `rate_limited`.
    pub fn acquire(
        &self,
        scope: &str,
        session_id: &str,
        rate: &RateLimit,
    ) -> Result<(), ErrorEnvelope> {
        let key = format!("{scope}:{session_id}");
        let entry = self.entry(&key, rate.burst);
        let queued = entry.queued.fetch_add(1, Ordering::SeqCst) + 1;
        let _slot = QueueSlot(&entry);
        if queued as u32 > rate.max_queued {
            let wait_ms = entry.bucket.lock().expect("bucket").wait(rate) * 1000.0;
            return Err(ErrorEnvelope::rate_limited_after(
                scope,
                wait_ms.ceil() as u64,
            ));
        }
        let _gate = entry.gate.lock().expect("rate gate");

        let (wait, available) = {
            let mut bucket = entry.bucket.lock().expect("bucket");
            bucket.refill(rate);
            (bucket.wait(rate), bucket.tokens >= 1.0)
        };
        if !available && rate.mode == crate::config::RateMode::Reject {
            return Err(ErrorEnvelope::rate_limited_after(
                scope,
                (wait * 1000.0).ceil() as u64,
            ));
        }
        if !available {
            let wait_ms = (wait * 1000.0).ceil() as u64;
            if wait_ms > rate.max_wait_ms {
                return Err(ErrorEnvelope::rate_limited_after(scope, wait_ms));
            }
            std::thread::sleep(std::time::Duration::from_millis(wait_ms));
        }
        let mut bucket = entry.bucket.lock().expect("bucket");
        bucket.refill(rate);
        bucket.take();
        Ok(())
    }
}

/// Decrements the parked count on every return path (errors included).
struct QueueSlot<'a>(&'a SessionRate);

impl Drop for QueueSlot<'_> {
    fn drop(&mut self) {
        self.0.queued.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RateMode;

    fn rate(per_sec: u32, burst: u32, mode: RateMode) -> RateLimit {
        RateLimit {
            per_sec,
            burst,
            mode,
            max_wait_ms: 30_000,
            max_queued: 64,
        }
    }

    #[test]
    fn a_burst_is_spent_then_rejected_with_a_retry_hint() {
        let limiter = RateLimiter::default();
        let rate = rate(1, 2, RateMode::Reject);
        assert!(limiter.acquire("s", "sess", &rate).is_ok());
        assert!(limiter.acquire("s", "sess", &rate).is_ok());
        let error = limiter.acquire("s", "sess", &rate).unwrap_err();
        assert_eq!(error.code, crate::ipc::ErrorCode::RateLimited);
        assert!(
            error
                .hint
                .as_deref()
                .unwrap_or_default()
                .starts_with("retry after "),
            "{error:?}"
        );
    }

    #[test]
    fn pace_waits_instead_of_rejecting() {
        let limiter = RateLimiter::default();
        let rate = rate(50, 1, RateMode::Pace);
        assert!(limiter.acquire("s", "sess", &rate).is_ok());
        let started = Instant::now();
        limiter.acquire("s", "sess", &rate).expect("paced");
        assert!(
            started.elapsed() >= std::time::Duration::from_millis(15),
            "second call waited: {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn the_queue_bound_refuses_instead_of_parking() {
        let limiter = Arc::new(RateLimiter::default());
        let mut rate = rate(1, 1, RateMode::Pace);
        rate.max_queued = 1;
        // Drain the bucket, then hold the gate with one parked caller while a second arrives.
        assert!(limiter.acquire("s", "sess2", &rate).is_ok());
        let first = {
            let limiter = Arc::clone(&limiter);
            let rate = rate.clone();
            std::thread::spawn(move || limiter.acquire("s", "sess2", &rate))
        };
        std::thread::sleep(std::time::Duration::from_millis(30));
        let second = limiter.acquire("s", "sess2", &rate).unwrap_err();
        assert_eq!(second.code, crate::ipc::ErrorCode::RateLimited);
        assert!(first.join().unwrap().is_ok());
    }

    #[test]
    fn the_wait_bound_refuses_a_long_park() {
        let limiter = RateLimiter::default();
        let mut rate = rate(1, 1, RateMode::Pace);
        rate.max_wait_ms = 10;
        assert!(limiter.acquire("s", "sess", &rate).is_ok());
        let error = limiter.acquire("s", "sess", &rate).unwrap_err();
        assert_eq!(error.code, crate::ipc::ErrorCode::RateLimited);
    }

    #[test]
    fn sessions_do_not_share_a_bucket() {
        let limiter = RateLimiter::default();
        let rate = rate(1, 1, RateMode::Reject);
        assert!(limiter.acquire("s", "one", &rate).is_ok());
        assert!(limiter.acquire("s", "two", &rate).is_ok());
    }
}
