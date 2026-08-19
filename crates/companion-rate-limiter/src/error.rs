//! Error types.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum RateLimiterError {
    #[error("invalid configuration: {0}")]
    InvalidConfig(&'static str),
    #[error("internal invariant violated: {0}")]
    Internal(&'static str),
}

impl RateLimiterError {
    pub const fn invalid(msg: &'static str) -> Self {
        Self::InvalidConfig(msg)
    }
}
