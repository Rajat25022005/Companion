//! # Companion Rate Limiter
//!
//! High-performance hierarchical rate limiter with three algorithm backends,
//! tiered quotas (per-IP → per-User → Global), and burst penalty cooldown.
//!
//! ## Algorithms
//!
//! - **Token Bucket** – lock-free CAS refill + consume. Best for steady throughput with burst tolerance.
//! - **Sliding Window Log** – fixed-capacity ring of timestamps; precise, no over-shoot near window boundaries.
//! - **Leaky Bucket** – drains queued "drips" at a constant rate; smooths traffic spikes.
//!
//! All algorithms are designed to be safe under high concurrent contention using atomics and
//! sharded state. They never block.
//!
//! ## Tiered quotas
//!
//! [`RateLimiter::check`] enforces a 3-tier hierarchy: the request must pass the IP scope,
//! the User scope, and the Global scope. Each tier is independent and configurable. Failing
//! any tier produces a [`Decision::Denied`] with the failing scope and retry-after hint.
//!
//! ## Burst penalty
//!
//! Repeated violations accrue a cooldown (`penalty_cooldown`). While in cooldown the caller
//! is denied with an exponentially growing backoff hint.

#![deny(unsafe_op_in_unsafe_fn)]
#![warn(missing_debug_implementations)]

pub mod algorithm;
pub mod config;
pub mod decision;
pub mod error;
pub mod scope;
pub mod state;
pub mod tier;

#[cfg(test)]
mod tests;

pub use algorithm::{LeakyBucket, RateLimitAlgo, SlidingWindowLog, TokenBucket};
pub use config::{LeakyBucketConfig, RateLimiterConfig, ScopeConfig, SlidingWindowConfig, TokenBucketConfig};
pub use decision::{Decision, DenyReason};
pub use error::RateLimiterError;
pub use scope::Scope;
pub use tier::{RateLimiter, RequestKey};
