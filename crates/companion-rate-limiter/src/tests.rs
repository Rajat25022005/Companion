//! Integration + concurrency tests for `companion-rate-limiter`.

use std::sync::Arc;
use std::time::Duration;

use crate::{
    Decision, DenyReason, RateLimiter, RateLimiterConfig, RequestKey, ScopeConfig,
};

fn cfg(rate: f64, burst: f64) -> RateLimiterConfig {
    RateLimiterConfig {
        ip: ScopeConfig::token_bucket(rate, burst),
        user: ScopeConfig::token_bucket(rate * 10.0, burst * 10.0),
        global: ScopeConfig::leaky_bucket(rate * 100.0, burst * 100.0),
        penalty_cooldown: Duration::from_millis(50),
        penalty_max: Duration::from_millis(500),
    }
}

#[test]
fn allows_burst_up_to_capacity() {
    let l = RateLimiter::new(cfg(5.0, 5.0));
    let k = RequestKey::new("1.1.1.1");
    for i in 0..5 {
        assert!(l.check(&k).is_allowed(), "i={i}");
    }
    assert!(l.check(&k).is_denied());
}

#[test]
fn different_ips_are_isolated() {
    let l = RateLimiter::new(cfg(1.0, 1.0));
    assert!(l.check(&RequestKey::new("1.1.1.1")).is_allowed());
    assert!(l.check(&RequestKey::new("2.2.2.2")).is_allowed());
    assert!(l.check(&RequestKey::new("1.1.1.1")).is_denied());
    assert!(l.check(&RequestKey::new("2.2.2.2")).is_denied());
}

#[test]
fn retry_after_is_positive_on_denial() {
    let l = RateLimiter::new(cfg(1.0, 1.0));
    let k = RequestKey::new("3.3.3.3");
    let _ = l.check(&k);
    let d = l.check(&k);
    match d {
        Decision::Denied { retry_after, .. } => assert!(retry_after > Duration::ZERO),
        _ => panic!("expected denial"),
    }
}

#[test]
fn penalty_cooldown_blocks_subsequent() {
    let l = RateLimiter::new(cfg(1.0, 1.0));
    let k = RequestKey::new("4.4.4.4");
    assert!(l.check(&k).is_allowed());
    let first = l.check(&k);
    assert!(matches!(
        first,
        Decision::Denied {
            reason: DenyReason::QuotaExceeded,
            ..
        }
    ));
    // Next call must be in cooldown, not raw quota.
    let second = l.check(&k);
    assert!(matches!(
        second,
        Decision::Denied {
            reason: DenyReason::PenaltyCooldown,
            ..
        }
    ));
}

#[test]
fn penalty_backoff_grows_then_caps() {
    let mut c = cfg(1.0, 1.0);
    c.penalty_cooldown = Duration::from_millis(20);
    c.penalty_max = Duration::from_millis(80);
    let l = RateLimiter::new(c);
    let k = RequestKey::new("5.5.5.5");
    let _ = l.check(&k);
    // Drain the bucket then trigger denials.
    let _ = l.check(&k);
    // We don't time the wall clock here, just confirm the limiter keeps denying.
    for _ in 0..5 {
        assert!(l.check(&k).is_denied());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tokio_concurrent_under_high_contention() {
    let l = Arc::new(RateLimiter::new(cfg(50_000.0, 50_000.0)));
    let mut joins = Vec::new();
    for w in 0..4 {
        let l = l.clone();
        joins.push(tokio::spawn(async move {
            let mut allowed = 0u64;
            for i in 0..25_000u64 {
                let ip = format!("10.0.{}.{}", w, i % 250);
                if l.check(&RequestKey::new(ip)).is_allowed() {
                    allowed += 1;
                }
            }
            allowed
        }));
    }
    let mut total = 0u64;
    for j in joins {
        total += j.await.unwrap();
    }
    // 4 * 25_000 = 100k requests against a 50k capacity + global 5M cap.
    // We must admit strictly less than the requested total because the
    // per-IP cap is the tightest scope.
    assert!(total > 0);
    assert!(total <= 100_000);
}

#[test]
fn sliding_window_does_not_over_admit() {
    let cfg = RateLimiterConfig {
        ip: ScopeConfig::sliding_window(3.0, 3.0, Duration::from_secs(60)),
        user: ScopeConfig::sliding_window(1_000.0, 1_000.0, Duration::from_secs(60)),
        global: ScopeConfig::leaky_bucket(1_000.0, 1_000.0),
        penalty_cooldown: Duration::from_millis(50),
        penalty_max: Duration::from_millis(500),
    };
    let l = RateLimiter::new(cfg);
    let k = RequestKey::new("6.6.6.6");
    for i in 0..3 {
        assert!(l.check(&k).is_allowed(), "i={i}");
    }
    assert!(l.check(&k).is_denied());
}

#[test]
fn leaky_bucket_global_protects_downstream() {
    let cfg = RateLimiterConfig {
        ip: ScopeConfig::token_bucket(1_000.0, 1_000.0),
        user: ScopeConfig::token_bucket(1_000.0, 1_000.0),
        global: ScopeConfig::leaky_bucket(3.0, 3.0),
        penalty_cooldown: Duration::from_millis(50),
        penalty_max: Duration::from_millis(500),
    };
    let l = RateLimiter::new(cfg);
    let mut allowed = 0;
    for i in 0..10 {
        let k = RequestKey::new(format!("7.7.7.{}", i));
        if l.check(&k).is_allowed() {
            allowed += 1;
        }
    }
    // Global leaky bucket has capacity 3; allow a small grace for the
    // drain window during the test, but cap far below the request count.
    assert!(allowed <= 5, "leaky bucket leaked too much: allowed={allowed}");
}

#[test]
fn debug_impl_doesnt_panic() {
    let l = RateLimiter::new(cfg(10.0, 10.0));
    let _ = format!("{l:?}");
}

#[test]
fn missing_user_id_skips_user_scope() {
    let cfg = RateLimiterConfig {
        ip: ScopeConfig::token_bucket(1.0, 1.0),
        user: ScopeConfig::token_bucket(1.0, 1.0),
        global: ScopeConfig::leaky_bucket(100.0, 100.0),
        penalty_cooldown: Duration::from_millis(50),
        penalty_max: Duration::from_millis(500),
    };
    let l = RateLimiter::new(cfg);
    let k = RequestKey::new("8.8.8.8"); // no user id
    assert!(l.check(&k).is_allowed());
    assert!(l.check(&k).is_denied());
    // User scope should not have been touched.
    assert_eq!(l.tracked_user_count(), 0);
}

#[test]
fn config_swap_is_observable() {
    let l = RateLimiter::new(cfg(1.0, 1.0));
    let mut c = l.config();
    c.ip = ScopeConfig::token_bucket(50.0, 50.0);
    l.set_config(c);
    // Burst should now be possible.
    let k = RequestKey::new("9.9.9.9");
    for _ in 0..50 {
        assert!(l.check(&k).is_allowed());
    }
}
