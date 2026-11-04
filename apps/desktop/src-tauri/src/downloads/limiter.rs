//! Shared bandwidth limit (token bucket, 1 second burst) for all downloads.

use std::sync::Mutex;
use std::time::Duration;

use tokio::time::Instant;

struct Bucket {
    rate: Option<u64>,
    tokens: f64,
    last: Instant,
}

pub struct RateLimiter {
    bucket: Mutex<Bucket>,
}

// Rates and chunk sizes are far below 2^52: f64 is exact enough.
#[allow(clippy::cast_precision_loss)]
impl RateLimiter {
    pub fn new(rate: Option<u64>) -> Self {
        Self {
            bucket: Mutex::new(Bucket {
                rate: rate.filter(|r| *r > 0),
                tokens: rate.map_or(0.0, |r| r as f64),
                last: Instant::now(),
            }),
        }
    }

    /// Bytes per second; `None` or 0 = unlimited.
    pub fn set_rate(&self, rate: Option<u64>) {
        let mut bucket = self
            .bucket
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let rate = rate.filter(|r| *r > 0);
        if bucket.rate != rate {
            bucket.rate = rate;
            bucket.tokens = rate.map_or(0.0, |r| r as f64);
            bucket.last = Instant::now();
        }
    }

    /// Waits until `bytes` may be transferred.
    pub async fn acquire(&self, bytes: u64) {
        let wait = {
            let mut bucket = self
                .bucket
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(rate) = bucket.rate else { return };
            let rate = rate as f64;
            let now = Instant::now();
            let elapsed = now.duration_since(bucket.last).as_secs_f64();
            bucket.last = now;
            bucket.tokens = (bucket.tokens + elapsed * rate).min(rate) - bytes as f64;
            if bucket.tokens < 0.0 {
                Duration::from_secs_f64(-bucket.tokens / rate)
            } else {
                Duration::ZERO
            }
        };
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn limits_to_the_configured_rate() {
        let limiter = RateLimiter::new(Some(1000));
        let start = Instant::now();
        for _ in 0..6 {
            limiter.acquire(500).await;
        }
        // 3000 bytes at 1000 B/s with a 1000 byte burst: about 2 seconds.
        let elapsed = start.elapsed().as_secs_f64();
        assert!((1.9..2.2).contains(&elapsed), "{elapsed}");

        limiter.set_rate(None);
        let start = Instant::now();
        limiter.acquire(10_000_000).await;
        assert!(start.elapsed() < Duration::from_millis(1));
    }
}
