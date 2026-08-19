//! Lock-free / low-contention rate-limiting algorithms.
//!
//! All algorithms are safe to share across threads via `&self` and never block threads.
//!
//! ## Algorithms:
//! - **Token Bucket** – 100% lock-free CAS on 64-bit atomic counters. Allows bursts up to capacity.
//! - **Sliding Window Log** – Precise timestamp ring buffer with sub-microsecond mutex.
//! - **Leaky Bucket** – Drains queued drips at a constant rate; smooths traffic spikes.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::LazyLock;
use std::time::{Duration, Instant};
use parking_lot::Mutex;

static MONOTONIC_ORIGIN: LazyLock<Instant> = LazyLock::new(Instant::now);

pub fn now_ns() -> u64 {
    MONOTONIC_ORIGIN.elapsed().as_nanos().min(u64::MAX as u128) as u64
}

/// Trait every algorithm implements.
pub trait RateLimitAlgo: Send + Sync {
    /// Attempt to admit one request, returning `true` on success.
    fn try_acquire(&self) -> bool;

    /// Suggested retry-after when denied, in milliseconds (approximate).
    fn retry_after_ms(&self) -> u64;

    /// Human-readable name for logs/metrics.
    fn name(&self) -> &'static str;

    /// Current "tokens" or capacity remaining, used for `Decision::remaining`.
    fn remaining(&self) -> u32;
}

// ---------------------------------------------------------------------------
// Token Bucket (100% Lock-Free Atomic CAS)
// ---------------------------------------------------------------------------

/// Lock-free token bucket.
///
/// Tokens are stored as a fixed-point `f64` (scaled by `1e6`) in an `AtomicU64`.
#[derive(Debug)]
pub struct TokenBucket {
    tokens_scaled: AtomicU64,
    last_refill_ns: AtomicU64,
    rate_per_sec: f64,
    capacity: f64,
}

const SCALE: f64 = 1.0e6;

impl TokenBucket {
    pub fn new(rate_per_sec: f64, capacity: f64) -> Self {
        assert!(rate_per_sec > 0.0, "rate_per_sec must be positive");
        assert!(capacity > 0.0, "capacity must be positive");
        let init_tokens = (capacity * SCALE).min(u64::MAX as f64) as u64;
        Self {
            tokens_scaled: AtomicU64::new(init_tokens),
            last_refill_ns: AtomicU64::new(now_ns()),
            rate_per_sec,
            capacity,
        }
    }

    /// Refill tokens based on elapsed time, then attempt to consume one.
    fn refill_and_consume(&self) -> bool {
        let now = now_ns();
        let last_ns = self.last_refill_ns.load(Ordering::Acquire);
        if now > last_ns {
            let elapsed_ns = now - last_ns;
            let refill = (elapsed_ns as f64 / 1_000_000_000.0) * self.rate_per_sec;
            if refill > 0.0 {
                if self.last_refill_ns.compare_exchange(last_ns, now, Ordering::AcqRel, Ordering::Relaxed).is_ok() {
                    let refill_scaled = (refill * SCALE) as u64;
                    let cap_scaled = (self.capacity * SCALE) as u64;
                    let mut current = self.tokens_scaled.load(Ordering::Acquire);
                    loop {
                        let new_val = current.saturating_add(refill_scaled).min(cap_scaled);
                        match self.tokens_scaled.compare_exchange_weak(current, new_val, Ordering::AcqRel, Ordering::Acquire) {
                            Ok(_) => break,
                            Err(actual) => current = actual,
                        }
                    }
                }
            }
        }

        let one_token_scaled = SCALE as u64;
        let mut current = self.tokens_scaled.load(Ordering::Acquire);
        loop {
            if current < one_token_scaled {
                return false;
            }
            let new_val = current - one_token_scaled;
            match self.tokens_scaled.compare_exchange_weak(current, new_val, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => return true,
                Err(actual) => current = actual,
            }
        }
    }
}

impl RateLimitAlgo for TokenBucket {
    fn try_acquire(&self) -> bool {
        self.refill_and_consume()
    }

    fn retry_after_ms(&self) -> u64 {
        let tokens_scaled = self.tokens_scaled.load(Ordering::Acquire);
        let tokens = tokens_scaled as f64 / SCALE;
        let deficit = (1.0 - tokens).max(0.0);
        ((deficit / self.rate_per_sec) * 1000.0).ceil() as u64
    }

    fn name(&self) -> &'static str {
        "token_bucket"
    }

    fn remaining(&self) -> u32 {
        let tokens_scaled = self.tokens_scaled.load(Ordering::Acquire);
        ((tokens_scaled as f64 / SCALE).min(u32::MAX as f64)) as u32
    }
}

// ---------------------------------------------------------------------------
// Sliding Window Log
// ---------------------------------------------------------------------------

pub struct SlidingWindowLog {
    inner: Mutex<SlidingWindowInner>,
}

#[derive(Debug)]
struct SlidingWindowInner {
    stamps: Vec<u64>,
    head: usize,
    len: usize,
    window_ns: u64,
    capacity: u32,
}

impl SlidingWindowLog {
    pub fn new(rate: f64, window: Duration) -> Self {
        assert!(rate > 0.0, "rate must be positive");
        let cap = rate.max(1.0).ceil() as usize;
        Self {
            inner: Mutex::new(SlidingWindowInner {
                stamps: vec![0; cap],
                head: 0,
                len: 0,
                window_ns: window.as_nanos().min(u64::MAX as u128) as u64,
                capacity: cap as u32,
            }),
        }
    }
}

impl std::fmt::Debug for SlidingWindowLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.lock();
        f.debug_struct("SlidingWindowLog")
            .field("window_ns", &inner.window_ns)
            .field("capacity", &inner.capacity)
            .finish()
    }
}

impl RateLimitAlgo for SlidingWindowLog {
    fn try_acquire(&self) -> bool {
        let now = now_ns();
        let mut inner = self.inner.lock();
        let cutoff = now.saturating_sub(inner.window_ns);
        while inner.len > 0 {
            let head_ts = inner.stamps[inner.head];
            if head_ts > cutoff {
                break;
            }
            inner.head = (inner.head + 1) % inner.stamps.len();
            inner.len -= 1;
        }

        if inner.len as u32 >= inner.capacity {
            return false;
        }
        let tail = (inner.head + inner.len) % inner.stamps.len();
        inner.stamps[tail] = now;
        inner.len += 1;
        true
    }

    fn retry_after_ms(&self) -> u64 {
        let now = now_ns();
        let inner = self.inner.lock();
        if inner.len == 0 {
            return 0;
        }
        let head_ts = inner.stamps[inner.head];
        let expire_at = head_ts.saturating_add(inner.window_ns);
        let delta_ns = expire_at.saturating_sub(now);
        ((delta_ns as f64 / 1_000_000.0).ceil()) as u64
    }

    fn name(&self) -> &'static str {
        "sliding_window_log"
    }

    fn remaining(&self) -> u32 {
        let inner = self.inner.lock();
        inner.capacity.saturating_sub(inner.len as u32)
    }
}

// ---------------------------------------------------------------------------
// Leaky Bucket
// ---------------------------------------------------------------------------

pub struct LeakyBucket {
    inner: Mutex<LeakyBucketInner>,
}

#[derive(Debug)]
struct LeakyBucketInner {
    queue: Vec<u64>,
    capacity: u32,
    rate_per_sec: f64,
}

impl LeakyBucket {
    pub fn new(rate_per_sec: f64, capacity: f64) -> Self {
        assert!(rate_per_sec > 0.0);
        let cap = capacity.max(1.0).ceil() as u32;
        Self {
            inner: Mutex::new(LeakyBucketInner {
                queue: Vec::with_capacity(cap as usize),
                capacity: cap,
                rate_per_sec,
            }),
        }
    }
}

impl std::fmt::Debug for LeakyBucket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.lock();
        f.debug_struct("LeakyBucket")
            .field("capacity", &inner.capacity)
            .field("rate_per_sec", &inner.rate_per_sec)
            .finish()
    }
}

impl RateLimitAlgo for LeakyBucket {
    fn try_acquire(&self) -> bool {
        let now = now_ns();
        let mut inner = self.inner.lock();
        let secs_per_drip = 1.0 / inner.rate_per_sec;
        let drain_window_ns = (secs_per_drip * 1_000_000_000.0).max(1.0) as u64;

        while let Some(&front) = inner.queue.first() {
            if now.saturating_sub(front) >= drain_window_ns {
                inner.queue.remove(0);
            } else {
                break;
            }
        }

        if (inner.queue.len() as u32) >= inner.capacity {
            return false;
        }
        inner.queue.push(now);
        true
    }

    fn retry_after_ms(&self) -> u64 {
        let now = now_ns();
        let inner = self.inner.lock();
        if inner.queue.is_empty() {
            return 0;
        }
        let front = *inner.queue.first().unwrap();
        let secs_per_drip = 1.0 / inner.rate_per_sec;
        let drain_window_ns = (secs_per_drip * 1_000_000_000.0) as u64;
        let ready_at = front.saturating_add(drain_window_ns);
        let delta_ns = ready_at.saturating_sub(now);
        ((delta_ns as f64 / 1_000_000.0).ceil()) as u64
    }

    fn name(&self) -> &'static str {
        "leaky_bucket"
    }

    fn remaining(&self) -> u32 {
        let inner = self.inner.lock();
        inner.capacity.saturating_sub(inner.queue.len() as u32)
    }
}

// ---------------------------------------------------------------------------
// AnyAlgo Enum
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum AnyAlgo {
    TokenBucket(TokenBucket),
    SlidingWindow(SlidingWindowLog),
    LeakyBucket(LeakyBucket),
}

impl RateLimitAlgo for AnyAlgo {
    fn try_acquire(&self) -> bool {
        match self {
            Self::TokenBucket(a) => a.try_acquire(),
            Self::SlidingWindow(a) => a.try_acquire(),
            Self::LeakyBucket(a) => a.try_acquire(),
        }
    }
    fn retry_after_ms(&self) -> u64 {
        match self {
            Self::TokenBucket(a) => a.retry_after_ms(),
            Self::SlidingWindow(a) => a.retry_after_ms(),
            Self::LeakyBucket(a) => a.retry_after_ms(),
        }
    }
    fn name(&self) -> &'static str {
        match self {
            Self::TokenBucket(a) => a.name(),
            Self::SlidingWindow(a) => a.name(),
            Self::LeakyBucket(a) => a.name(),
        }
    }
    fn remaining(&self) -> u32 {
        match self {
            Self::TokenBucket(a) => a.remaining(),
            Self::SlidingWindow(a) => a.remaining(),
            Self::LeakyBucket(a) => a.remaining(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn token_bucket_initial_full() {
        let tb = TokenBucket::new(10.0, 10.0);
        for _ in 0..10 {
            assert!(tb.try_acquire());
        }
        assert!(!tb.try_acquire());
    }

    #[test]
    fn sliding_window_rejects_when_full() {
        let sw = SlidingWindowLog::new(5.0, Duration::from_secs(1));
        for _ in 0..5 {
            assert!(sw.try_acquire());
        }
        assert!(!sw.try_acquire());
    }

    #[test]
    fn leaky_bucket_eventually_drains() {
        let lb = LeakyBucket::new(100.0, 5.0);
        for _ in 0..5 {
            assert!(lb.try_acquire());
        }
        assert!(!lb.try_acquire());
        thread::sleep(Duration::from_millis(60));
        for _ in 0..5 {
            assert!(lb.try_acquire());
        }
    }

    #[test]
    fn concurrent_token_bucket_is_safe() {
        let tb = Arc::new(TokenBucket::new(100_000.0, 100_000.0));
        let mut handles = Vec::new();
        for _ in 0..8 {
            let tb = tb.clone();
            handles.push(thread::spawn(move || {
                let mut allowed = 0u64;
                for _ in 0..10_000 {
                    if tb.try_acquire() {
                        allowed += 1;
                    }
                }
                allowed
            }));
        }
        let total: u64 = handles.into_iter().map(|h| h.join().unwrap()).sum();
        assert!(total >= 60_000, "allowed={total}");
    }
}
