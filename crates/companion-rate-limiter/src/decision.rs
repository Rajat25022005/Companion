//! Rate limiter decision types.

use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::scope::Scope;

/// Why a request was denied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DenyReason {
    /// Hard limit reached on the indicated scope.
    QuotaExceeded,
    /// Caller is in a penalty cooldown window from prior violations.
    PenaltyCooldown,
}

impl DenyReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::QuotaExceeded => "quota_exceeded",
            Self::PenaltyCooldown => "penalty_cooldown",
        }
    }
}

/// Decision returned from [`crate::RateLimiter::check`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decision {
    /// Request is allowed. Carries remaining capacity in the *tightest* scope.
    Allowed { remaining: u32 },
    /// Request is denied.
    Denied {
        scope: Scope,
        reason: DenyReason,
        /// Suggested time the caller should wait before retrying.
        retry_after: Duration,
    },
}

impl Decision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allowed { .. })
    }

    pub fn is_denied(&self) -> bool {
        matches!(self, Self::Denied { .. })
    }
}
