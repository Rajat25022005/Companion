//! Rate limiter configuration types.
//!
//! Defaults are tuned for a typical API gateway use case (100 req/s/IP, 1000 req/s/user,
//! 10_000 req/s/global, burst capacity = rate, 5s penalty cooldown).

use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::algorithm::{LeakyBucket, SlidingWindowLog, TokenBucket};
use crate::scope::Scope;

/// Parameters shared by every algorithm.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BaseQuota {
    /// Steady-state requests-per-second allowed.
    pub rate_per_sec: f64,
    /// Maximum burst capacity (algorithm-specific semantics).
    pub burst: f64,
}

impl BaseQuota {
    pub const fn new(rate_per_sec: f64, burst: f64) -> Self {
        Self {
            rate_per_sec,
            burst,
        }
    }
}

/// Configuration for [`TokenBucket`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TokenBucketConfig {
    pub base: BaseQuota,
}

impl TokenBucketConfig {
    pub const fn new(rate_per_sec: f64, burst: f64) -> Self {
        Self {
            base: BaseQuota::new(rate_per_sec, burst),
        }
    }
}

/// Configuration for [`SlidingWindowLog`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SlidingWindowConfig {
    pub base: BaseQuota,
    /// Window length. Defaults to 1 second; the `rate_per_sec` is interpreted as
    /// "requests allowed within this window".
    pub window: Duration,
}

impl SlidingWindowConfig {
    pub const fn new(rate_per_sec: f64, burst: f64, window: Duration) -> Self {
        Self {
            base: BaseQuota::new(rate_per_sec, burst),
            window,
        }
    }
}

/// Configuration for [`LeakyBucket`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct LeakyBucketConfig {
    pub base: BaseQuota,
}

impl LeakyBucketConfig {
    pub const fn new(rate_per_sec: f64, burst: f64) -> Self {
        Self {
            base: BaseQuota::new(rate_per_sec, burst),
        }
    }
}

/// Per-scope configuration: which algorithm to run and at what quota.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ScopeConfig {
    TokenBucket(TokenBucketConfig),
    SlidingWindow(SlidingWindowConfig),
    LeakyBucket(LeakyBucketConfig),
}

impl ScopeConfig {
    /// Convenience constructor mirroring the defaults of a Token Bucket.
    pub const fn token_bucket(rate_per_sec: f64, burst: f64) -> Self {
        Self::TokenBucket(TokenBucketConfig::new(rate_per_sec, burst))
    }

    /// Convenience constructor for a Sliding Window Log.
    pub const fn sliding_window(rate_per_sec: f64, burst: f64, window: Duration) -> Self {
        Self::SlidingWindow(SlidingWindowConfig::new(rate_per_sec, burst, window))
    }

    /// Convenience constructor for a Leaky Bucket.
    pub const fn leaky_bucket(rate_per_sec: f64, burst: f64) -> Self {
        Self::LeakyBucket(LeakyBucketConfig::new(rate_per_sec, burst))
    }

    /// Returns the shared base quota, used by the penalty tracker and metrics.
    pub fn base(&self) -> BaseQuota {
        match self {
            Self::TokenBucket(c) => c.base,
            Self::SlidingWindow(c) => c.base,
            Self::LeakyBucket(c) => c.base,
        }
    }

    /// Builds a fresh algorithm instance for a single scope-state slot.
    pub fn instantiate(&self) -> crate::algorithm::AnyAlgo {
        match self {
            Self::TokenBucket(c) => crate::algorithm::AnyAlgo::TokenBucket(TokenBucket::new(
                c.base.rate_per_sec,
                c.base.burst,
            )),
            Self::SlidingWindow(c) => {
                crate::algorithm::AnyAlgo::SlidingWindow(SlidingWindowLog::new(
                    c.base.rate_per_sec,
                    c.window,
                ))
            }
            Self::LeakyBucket(c) => crate::algorithm::AnyAlgo::LeakyBucket(LeakyBucket::new(
                c.base.rate_per_sec,
                c.base.burst,
            )),
        }
    }
}

/// Top-level configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimiterConfig {
    /// Per-IP scope configuration (always evaluated; falls back to global if missing).
    pub ip: ScopeConfig,
    /// Per-User scope configuration (skipped when no `user_id` is provided).
    pub user: ScopeConfig,
    /// Global fallback configuration (always evaluated last).
    pub global: ScopeConfig,
    /// Cooldown applied after a denial to penalize bursts.
    pub penalty_cooldown: Duration,
    /// Exponential backoff cap applied to repeat offenders.
    pub penalty_max: Duration,
}

impl Default for RateLimiterConfig {
    fn default() -> Self {
        Self {
            ip: ScopeConfig::token_bucket(100.0, 100.0),
            user: ScopeConfig::token_bucket(1_000.0, 1_000.0),
            global: ScopeConfig::leaky_bucket(10_000.0, 10_000.0),
            penalty_cooldown: Duration::from_secs(5),
            penalty_max: Duration::from_secs(60),
        }
    }
}

impl RateLimiterConfig {
    /// Returns the [`ScopeConfig`] for a particular scope level.
    pub fn for_scope(&self, scope: Scope) -> &ScopeConfig {
        match scope {
            Scope::Ip => &self.ip,
            Scope::User => &self.user,
            Scope::Global => &self.global,
        }
    }
}
