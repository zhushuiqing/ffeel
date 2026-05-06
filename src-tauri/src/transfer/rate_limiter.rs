#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_new_has_full_tokens() {
        let limiter = RateLimiter::new(1024);
        assert_eq!(limiter.capacity, 1024);
        assert_eq!(limiter.tokens, 1024.0);
        assert_eq!(limiter.refill_rate, 1024.0);
    }

    #[tokio::test]
    async fn test_consume_within_capacity() {
        let mut limiter = RateLimiter::new(1024);
        // 应即时消耗，无需等待
        limiter.consume(512).await;
        assert!(limiter.tokens <= 512.0);
    }

    #[tokio::test]
    async fn test_zero_speed_limit() {
        let mut limiter = RateLimiter::new(0);
        // 不限速时应即时返回
        limiter.consume(999999).await;
    }

    #[test]
    fn test_refill_over_time() {
        let mut limiter = RateLimiter::new(100);
        limiter.tokens = 10.0;
        limiter.last_refill = Instant::now() - Duration::from_secs(1);
        limiter.refill();
        // 1 秒应补充 100 个令牌，但上限 100
        assert!((limiter.tokens - 100.0).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_consume_exceeding_capacity_waits() {
        let mut limiter = RateLimiter::new(100);
        let start = Instant::now();
        // 消耗 200，需要等待约 1 秒
        limiter.consume(200).await;
        let elapsed = start.elapsed();
        assert!(elapsed >= Duration::from_millis(900));
    }
}

use std::time::Instant;

/// 令牌桶速率限制器
pub struct RateLimiter {
    capacity: u64,
    tokens: f64,
    refill_rate: f64,
    last_refill: Instant,
}

impl RateLimiter {
    pub fn new(bytes_per_sec: u64) -> Self {
        Self {
            capacity: bytes_per_sec,
            tokens: bytes_per_sec as f64,
            refill_rate: bytes_per_sec as f64,
            last_refill: Instant::now(),
        }
    }

    /// 消耗 n 个字节的令牌，必要时等待（每 200ms 轮询，可被取消）
    pub async fn consume(&mut self, n: u64) {
        if self.refill_rate <= 0.0 {
            return;
        }
        self.refill();
        if self.tokens >= n as f64 {
            self.tokens -= n as f64;
            return;
        }
        let deficit = n as f64 - self.tokens;
        let wait_secs = deficit / self.refill_rate;
        if wait_secs > 0.0 {
            // 分段等待，避免阻塞取消/暂停检测
            let poll_interval = tokio::time::Duration::from_millis(200);
            let total_waits = (wait_secs / 0.2).ceil() as u32;
            for _ in 0..total_waits {
                tokio::time::sleep(poll_interval).await;
                self.refill();
                if self.tokens >= n as f64 {
                    break;
                }
            }
        }
        self.tokens = (self.tokens.min(n as f64) + deficit).min(self.capacity as f64);
        self.tokens = (self.tokens - n as f64).max(0.0);
    }

    fn refill(&mut self) {
        let elapsed = self.last_refill.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.capacity as f64);
            self.last_refill = Instant::now();
        }
    }
}
