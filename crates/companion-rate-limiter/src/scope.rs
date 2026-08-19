//! Scope levels in the hierarchical limiter.

use serde::{Deserialize, Serialize};

/// Scope level. Order indicates evaluation order: `Ip` → `User` → `Global`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Scope {
    Ip = 0,
    User = 1,
    Global = 2,
}

impl Scope {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ip => "ip",
            Self::User => "user",
            Self::Global => "global",
        }
    }
}
