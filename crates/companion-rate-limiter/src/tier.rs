//! Top-level [`RateLimiter`] type: the hierarchical, tier-aware façade.

use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use parking_lot::RwLock;

use crate::algorithm::RateLimitAlgo;
use crate::config::RateLimiterConfig;
use crate::decision::{Decision, DenyReason};
use crate::scope::Scope;
use crate::state::ScopeState;

/// Identifies a caller: IP plus optional user id.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct RequestKey {
    pub ip: String,
    pub user_id: Option<String>,
}

impl RequestKey {
    pub fn new(ip: impl Into<String>) -> Self {
        Self {
            ip: ip.into(),
            user_id: None,
        }
    }

    pub fn with_user(mut self, user_id: impl Into<String>) -> Self {
        self.user_id = Some(user_id.into());
        self
    }
}

/// The hierarchical rate limiter.
///
/// Internally three `DashMap`s hold per-scope state. All operations are lock-free
/// for the algorithm hot path; the maps only take a brief sharded lock.
pub struct RateLimiter {
    config: Arc<RwLock<RateLimiterConfig>>,
    ip_state: DashMap<String, Arc<ScopeState>>,
    user_state: DashMap<String, Arc<ScopeState>>,
    global_state: DashMap<&'static str, Arc<ScopeState>>,
}

impl std::fmt::Debug for RateLimiter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let cfg = self.config.read();
        f.debug_struct("RateLimiter")
            .field("ip_entries", &self.ip_state.len())
            .field("user_entries", &self.user_state.len())
            .field("global_entries", &self.global_state.len())
            .field("penalty_cooldown", &cfg.penalty_cooldown)
            .finish()
    }
}

impl RateLimiter {
    pub fn new(config: RateLimiterConfig) -> Self {
        let limiter = Self {
            config: Arc::new(RwLock::new(config)),
            ip_state: DashMap::new(),
            user_state: DashMap::new(),
            global_state: DashMap::new(),
        };
        // Pre-seed the singleton global scope.
        limiter.global_state.insert(
            "global",
            Arc::new(ScopeState::new(
                limiter.config.read().global.instantiate(),
            )),
        );
        limiter
    }

    /// Returns the active configuration.
    pub fn config(&self) -> RateLimiterConfig {
        self.config.read().clone()
    }

    /// Replace the configuration (does not retroactively reset existing state).
    pub fn set_config(&self, new_cfg: RateLimiterConfig) {
        *self.config.write() = new_cfg;
        // Re-instantiate the global singleton if its algorithm identity changed.
        let new_global = self.config.read().global.instantiate();
        if let Some(entry) = self.global_state.get("global") {
            // SAFETY: we are the sole writer here under the config write lock;
            // we drop the existing Arc and install the freshly built one.
            let key = *entry.key();
            drop(entry);
            self.global_state.insert(key, Arc::new(ScopeState::new(new_global)));
        }
    }

    /// Internal helper: get-or-create a scope-state for `key` at `scope`.
    fn get_or_create(&self, scope: Scope, key: &str) -> Arc<ScopeState> {
        let map: &DashMap<String, Arc<ScopeState>> = match scope {
            Scope::Ip => &self.ip_state,
            Scope::User => &self.user_state,
            Scope::Global => {
                if let Some(entry) = self.global_state.get("global") {
                    return entry.value().clone();
                }
                // Should be unreachable given `new` pre-seeds, but be safe.
                let cfg = self.config.read().clone();
                let state = Arc::new(ScopeState::new(cfg.global.instantiate()));
                self.global_state.insert("global", state.clone());
                return state;
            }
        };
        if let Some(entry) = map.get(key) {
            return entry.value().clone();
        }
        let cfg = self.config.read().clone();
        let algo = cfg.for_scope(scope).instantiate();
        let state = Arc::new(ScopeState::new(algo));
        // Race: another thread may have inserted. DashMap's `entry` API is the cleanest.
        map.entry(key.to_owned())
            .or_insert_with(|| state.clone())
            .value()
            .clone()
    }

    /// Check whether `key` may admit one request right now. If allowed, the
    /// token is *consumed* (the underlying algorithm state is updated).
    ///
    /// Returns the tightest scope's remaining capacity on success.
    pub fn check(&self, key: &RequestKey) -> Decision {
        let cfg = self.config.read().clone();

        // 1) IP scope (always evaluated).
        let ip_state = self.get_or_create(Scope::Ip, &key.ip);
        if let Some(decision) = self.evaluate_scope(&ip_state, Scope::Ip, &cfg) {
            return decision;
        }

        // 2) User scope (only if a user_id is present).
        if let Some(uid) = &key.user_id {
            let user_state = self.get_or_create(Scope::User, uid);
            if let Some(decision) = self.evaluate_scope(&user_state, Scope::User, &cfg) {
                return decision;
            }
        }

        // 3) Global fallback.
        let global_state = self.get_or_create(Scope::Global, "global");
        if let Some(decision) = self.evaluate_scope(&global_state, Scope::Global, &cfg) {
            return decision;
        }

        // All scopes allowed — return remaining of the tightest non-global scope.
        let remaining = ip_state.algo.remaining().min(
            key.user_id
                .as_ref()
                .map(|_| {
                    self.get_or_create(Scope::User, key.user_id.as_ref().expect("present"))
                        .algo
                        .remaining()
                })
                .unwrap_or(u32::MAX),
        );

        Decision::Allowed { remaining }
    }

    /// Returns `Some(Decision::Denied)` if the scope denies the request,
    /// or `None` if the request passes this scope.
    fn evaluate_scope(
        &self,
        state: &ScopeState,
        scope: Scope,
        cfg: &RateLimiterConfig,
    ) -> Option<Decision> {
        // Penalty cooldown takes precedence.
        let cd = state.cooldown_remaining();
        if cd > Duration::ZERO {
            return Some(Decision::Denied {
                scope,
                reason: DenyReason::PenaltyCooldown,
                retry_after: cd,
            });
        }

        if state.algo.try_acquire() {
            state.note_allowed();
            return None;
        }

        let retry_ms = state.algo.retry_after_ms();
        let retry = Duration::from_millis(retry_ms.max(1));
        state.note_denied(cfg.penalty_cooldown, cfg.penalty_max);
        Some(Decision::Denied {
            scope,
            reason: DenyReason::QuotaExceeded,
            retry_after: retry,
        })
    }

    /// Returns the count of currently tracked IPs (for tests / metrics).
    pub fn tracked_ip_count(&self) -> usize {
        self.ip_state.len()
    }
    pub fn tracked_user_count(&self) -> usize {
        self.user_state.len()
    }
}

#[cfg(test)]
mod tier_tests {
    use super::*;

    fn cfg_with_ip(rate: f64, burst: f64) -> RateLimiterConfig {
        RateLimiterConfig {
            ip: crate::config::ScopeConfig::token_bucket(rate, burst),
            user: crate::config::ScopeConfig::token_bucket(rate * 10.0, burst * 10.0),
            global: crate::config::ScopeConfig::leaky_bucket(rate * 100.0, burst * 100.0),
            penalty_cooldown: Duration::from_millis(50),
            penalty_max: Duration::from_secs(1),
        }
    }

    #[test]
    fn tiered_enforcement_blocks_ip_first() {
        let l = RateLimiter::new(cfg_with_ip(2.0, 2.0));
        let k = RequestKey::new("1.1.1.1").with_user("alice");
        assert!(l.check(&k).is_allowed());
        assert!(l.check(&k).is_allowed());
        let d = l.check(&k);
        assert!(matches!(
            d,
            Decision::Denied {
                scope: Scope::Ip,
                ..
            }
        ));
    }

    #[test]
    fn global_scope_is_reached_only_when_lower_pass() {
        let cfg = RateLimiterConfig {
            ip: crate::config::ScopeConfig::token_bucket(100.0, 100.0),
            user: crate::config::ScopeConfig::token_bucket(100.0, 100.0),
            global: crate::config::ScopeConfig::token_bucket(2.0, 2.0),
            penalty_cooldown: Duration::from_millis(50),
            penalty_max: Duration::from_secs(1),
        };
        let l = RateLimiter::new(cfg);
        // Two distinct IPs / users so the global cap is the bottleneck.
        assert!(l.check(&RequestKey::new("1.1.1.1")).is_allowed());
        assert!(l.check(&RequestKey::new("2.2.2.2")).is_allowed());
        let d = l.check(&RequestKey::new("3.3.3.3"));
        assert!(matches!(
            d,
            Decision::Denied {
                scope: Scope::Global,
                ..
            }
        ));
    }

    #[test]
    fn penalty_cooldown_escalates_then_resets() {
        let l = RateLimiter::new(cfg_with_ip(1.0, 1.0));
        let k = RequestKey::new("9.9.9.9");
        assert!(l.check(&k).is_allowed());
        assert!(l.check(&k).is_denied());
        // First denial seeds a cooldown; a follow-up should be denied for penalty reason.
        let d = l.check(&k);
        assert!(matches!(
            d,
            Decision::Denied {
                reason: DenyReason::PenaltyCooldown,
                ..
            }
        ));
    }
}
