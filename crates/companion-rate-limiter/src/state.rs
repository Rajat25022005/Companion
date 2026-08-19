//! Per-scope state stored in the limiter's `DashMap`.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

use crate::algorithm::{now_ns, AnyAlgo, RateLimitAlgo};
use crate::scope::Scope;

/// State attached to every (scope, key) pair.
pub struct ScopeState {
    /// Underlying rate-limit algorithm.
    pub algo: AnyAlgo,
    /// Number of recent denials used by the burst-penalty backoff.
    pub denial_streak: AtomicU32,
    /// Monotonic nanos at which the penalty cooldown expires (0 = no cooldown).
    pub cooldown_until_ns: AtomicU64,
    /// Last time we touched this scope-state (for LRU eviction; not yet wired).
    pub last_used_ns: AtomicU64,
}

impl ScopeState {
    pub fn new(algo: AnyAlgo) -> Self {
        Self {
            algo,
            denial_streak: AtomicU32::new(0),
            cooldown_until_ns: AtomicU64::new(0),
            last_used_ns: AtomicU64::new(now_ns()),
        }
    }

    pub fn touch(&self) {
        self.last_used_ns.store(now_ns(), Ordering::Relaxed);
    }

    pub fn cooldown_remaining(&self) -> Duration {
        let until = self.cooldown_until_ns.load(Ordering::Acquire);
        if until == 0 {
            return Duration::ZERO;
        }
        let now = now_ns();
        if now >= until {
            Duration::ZERO
        } else {
            Duration::from_nanos(until - now)
        }
    }

    pub fn in_cooldown(&self) -> bool {
        self.cooldown_remaining() > Duration::ZERO
    }

    pub fn note_allowed(&self) {
        self.denial_streak.store(0, Ordering::Release);
        self.cooldown_until_ns.store(0, Ordering::Release);
        self.touch();
    }

    pub fn note_denied(&self, base_cooldown: Duration, cap: Duration) {
        // Exponential growth, capped.
        let streak = self.denial_streak.fetch_add(1, Ordering::AcqRel) + 1;
        let factor = 1u32 << streak.min(10); // cap factor at 1024
        let cd = base_cooldown.saturating_mul(factor).min(cap);
        let until = now_ns() + cd.as_nanos() as u64;
        self.cooldown_until_ns.store(until, Ordering::Release);
    }
}

impl std::fmt::Debug for ScopeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScopeState")
            .field("algo", &self.algo.name())
            .field("denial_streak", &self.denial_streak.load(Ordering::Relaxed))
            .field("in_cooldown", &self.in_cooldown())
            .finish()
    }
}

/// Marker trait for keys stored in the hierarchical limiter.
pub trait ScopeKey {
    fn scope(&self) -> Scope;
}
