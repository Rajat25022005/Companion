//! Throughput benchmarks under Tokio concurrency.
//!
//! Run with: `cargo bench --manifest-path crates/companion-rate-limiter/Cargo.toml`

use std::sync::Arc;
use std::time::{Duration, Instant};

use companion_rate_limiter::{RateLimiter, RateLimiterConfig, RequestKey, ScopeConfig};

fn high_capacity_config() -> RateLimiterConfig {
    RateLimiterConfig {
        ip: ScopeConfig::token_bucket(1_000_000.0, 1_000_000.0),
        user: ScopeConfig::token_bucket(1_000_000.0, 1_000_000.0),
        global: ScopeConfig::leaky_bucket(1_000_000.0, 1_000_000.0),
        penalty_cooldown: Duration::from_millis(50),
        penalty_max: Duration::from_millis(500),
    }
}

/// Drives `n_workers` async tasks that each call `check()` `per_worker` times
/// against unique IPs, then prints aggregate throughput.
async fn run_bench(label: &str, n_workers: usize, per_worker: u64) {
    let l = Arc::new(RateLimiter::new(high_capacity_config()));
    let mut joins = Vec::with_capacity(n_workers);
    let started = Instant::now();
    for w in 0..n_workers {
        let l = l.clone();
        joins.push(tokio::spawn(async move {
            let mut allowed = 0u64;
            for i in 0..per_worker {
                let ip = format!("10.0.{}.{}", w, i);
                if l.check(&RequestKey::new(ip)).is_allowed() {
                    allowed += 1;
                }
            }
            allowed
        }));
    }
    let mut total_allowed = 0u64;
    for j in joins {
        total_allowed += j.await.unwrap();
    }
    let elapsed = started.elapsed();
    let total_ops = n_workers as u64 * per_worker;
    let ops_per_sec = total_ops as f64 / elapsed.as_secs_f64();
    println!(
        "[{label}] workers={n_workers} ops={total_ops} allowed={total_allowed} elapsed={:?} throughput={:.0} ops/s",
        elapsed,
        ops_per_sec
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn bench_high_contention_8_workers() {
    run_bench("tokio-8w", 8, 50_000).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 16)]
async fn bench_high_contention_16_workers() {
    run_bench("tokio-16w", 16, 50_000).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bench_single_ip_contention() {
    // Hammer the same IP from 4 workers — exercises the DashMap hot slot.
    let l = Arc::new(RateLimiter::new(high_capacity_config()));
    let started = Instant::now();
    let mut joins = Vec::new();
    for _ in 0..4 {
        let l = l.clone();
        joins.push(tokio::spawn(async move {
            let mut allowed = 0u64;
            for _ in 0..100_000u64 {
                if l.check(&RequestKey::new("203.0.113.1")).is_allowed() {
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
    let elapsed = started.elapsed();
    println!(
        "[single-ip] total={total} elapsed={:?} throughput={:.0} ops/s",
        elapsed,
        400_000.0 / elapsed.as_secs_f64()
    );
}
