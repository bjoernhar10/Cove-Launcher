//! Minimal async rate limiter: at most one request per `interval`, in order.

use std::time::Duration;

use tokio::sync::Mutex;
use tokio::time::Instant;

#[derive(Debug)]
pub struct RateLimiter {
    interval: Duration,
    next_slot: Mutex<Instant>,
}

impl RateLimiter {
    /// `per_second` requests per second (IGDB allows 4, RAWG we keep at 5).
    pub fn per_second(per_second: u32) -> Self {
        Self::with_interval(Duration::from_secs(1) / per_second.max(1))
    }

    pub fn with_interval(interval: Duration) -> Self {
        Self { interval, next_slot: Mutex::new(Instant::now()) }
    }

    /// Waits until the caller may send the next request.
    pub async fn acquire(&self) {
        let wait_until = {
            let mut next = self.next_slot.lock().await;
            let now = Instant::now();
            let slot = (*next).max(now);
            *next = slot + self.interval;
            slot
        };
        tokio::time::sleep_until(wait_until).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn spaces_requests_evenly() {
        let limiter = RateLimiter::per_second(4);
        let start = Instant::now();
        for _ in 0..5 {
            limiter.acquire().await;
        }
        // 5 requests at 4/s: the fifth starts after 4 × 250 ms.
        assert_eq!(start.elapsed(), Duration::from_millis(1000));
    }
}
