use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use tracing::{warn, debug};

use companion_domain::{RateLimitPolicy, ToolError};

/// Circuit breaker state for a capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

struct CapabilityCircuit {
    state: CircuitState,
    consecutive_failures: u32,
    last_state_change: Instant,
}

impl CapabilityCircuit {
    fn new() -> Self {
        Self {
            state: CircuitState::Closed,
            consecutive_failures: 0,
            last_state_change: Instant::now(),
        }
    }
}

struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
    active_concurrent: u32,
}

impl TokenBucket {
    fn new(max_tokens: f64) -> Self {
        Self {
            tokens: max_tokens,
            last_refill: Instant::now(),
            active_concurrent: 0,
        }
    }
}

/// Rate limiter and circuit breaker manager for registered capabilities.
#[derive(Clone)]
pub struct RateLimiter {
    buckets: Arc<Mutex<HashMap<String, TokenBucket>>>,
    circuits: Arc<Mutex<HashMap<String, CapabilityCircuit>>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            buckets: Arc::new(Mutex::new(HashMap::new())),
            circuits: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Check if an invocation is permitted by rate limit and circuit breaker.
    pub async fn check_and_acquire(
        &self,
        capability_name: &str,
        policy: &RateLimitPolicy,
    ) -> Result<(), ToolError> {
        let key = capability_name.to_lowercase();

        // 1. Circuit Breaker Check
        {
            let mut circuits = self.circuits.lock().await;
            let circuit = circuits.entry(key.clone()).or_insert_with(CapabilityCircuit::new);

            match circuit.state {
                CircuitState::Open => {
                    let elapsed = circuit.last_state_change.elapsed().as_secs();
                    if elapsed >= policy.cooldown_seconds {
                        debug!(capability = %capability_name, "circuit breaker transitioning to HalfOpen probe");
                        circuit.state = CircuitState::HalfOpen;
                        circuit.last_state_change = Instant::now();
                    } else {
                        warn!(
                            capability = %capability_name,
                            cooldown_remaining_secs = policy.cooldown_seconds - elapsed,
                            "invocation blocked by OPEN circuit breaker"
                        );
                        return Err(ToolError {
                            tool_call_id: String::new(),
                            message: format!(
                                "Circuit breaker for `{capability_name}` is OPEN (cooldown {}s remaining)",
                                policy.cooldown_seconds - elapsed
                            ),
                            retryable: true,
                        });
                    }
                }
                CircuitState::HalfOpen | CircuitState::Closed => {}
            }
        }

        // 2. Token Bucket Rate Limiting & Concurrency
        {
            let mut buckets = self.buckets.lock().await;
            let max_tokens = policy.requests_per_minute as f64;
            let refill_rate_per_sec = max_tokens / 60.0;

            let bucket = buckets.entry(key).or_insert_with(|| TokenBucket::new(max_tokens));

            // Check concurrency limit
            if bucket.active_concurrent >= policy.max_concurrent {
                warn!(
                    capability = %capability_name,
                    active = bucket.active_concurrent,
                    limit = policy.max_concurrent,
                    "concurrency limit reached"
                );
                return Err(ToolError {
                    tool_call_id: String::new(),
                    message: format!("Concurrency limit ({}) reached for `{capability_name}`", policy.max_concurrent),
                    retryable: true,
                });
            }

            // Refill tokens
            let now = Instant::now();
            let elapsed_secs = now.duration_since(bucket.last_refill).as_secs_f64();
            bucket.tokens = (bucket.tokens + elapsed_secs * refill_rate_per_sec).min(max_tokens);
            bucket.last_refill = now;

            if bucket.tokens < 1.0 {
                warn!(capability = %capability_name, "rate limit exceeded");
                return Err(ToolError {
                    tool_call_id: String::new(),
                    message: format!("Rate limit ({} req/min) exceeded for `{capability_name}`", policy.requests_per_minute),
                    retryable: true,
                });
            }

            bucket.tokens -= 1.0;
            bucket.active_concurrent += 1;
        }

        Ok(())
    }

    /// Record execution completion and update circuit breaker stats.
    pub async fn record_outcome(
        &self,
        capability_name: &str,
        policy: &RateLimitPolicy,
        success: bool,
    ) {
        let key = capability_name.to_lowercase();

        // Release concurrent counter
        {
            let mut buckets = self.buckets.lock().await;
            if let Some(bucket) = buckets.get_mut(&key) {
                bucket.active_concurrent = bucket.active_concurrent.saturating_sub(1);
            }
        }

        // Update circuit state
        {
            let mut circuits = self.circuits.lock().await;
            let circuit = circuits.entry(key.clone()).or_insert_with(CapabilityCircuit::new);

            if success {
                if circuit.state == CircuitState::HalfOpen {
                    debug!(capability = %capability_name, "probe succeeded, closing circuit breaker");
                    circuit.state = CircuitState::Closed;
                    circuit.consecutive_failures = 0;
                    circuit.last_state_change = Instant::now();
                } else if circuit.state == CircuitState::Closed {
                    circuit.consecutive_failures = 0;
                }
            } else {
                circuit.consecutive_failures += 1;
                if circuit.consecutive_failures >= policy.circuit_breaker_threshold {
                    warn!(
                        capability = %capability_name,
                        failures = circuit.consecutive_failures,
                        threshold = policy.circuit_breaker_threshold,
                        "tripping circuit breaker to OPEN"
                    );
                    circuit.state = CircuitState::Open;
                    circuit.last_state_change = Instant::now();
                }
            }
        }
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}
