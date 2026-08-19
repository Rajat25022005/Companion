use std::collections::HashMap;
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;

/// Cache tracker for stable prompt prefixes to accelerate LLM prompt caching.
#[derive(Debug, Default)]
pub struct ContextCache {
    /// Cached prefix fingerprints: hash -> hit count
    fingerprints: RwLock<HashMap<String, u64>>,
}

impl ContextCache {
    pub fn new() -> Self {
        Self {
            fingerprints: RwLock::new(HashMap::new()),
        }
    }

    /// Compute deterministic SHA256 fingerprint for a stable context prefix.
    pub fn compute_fingerprint(prefix: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(prefix.as_bytes());
        let result = hasher.finalize();
        format!("{:x}", result)
    }

    /// Record a cache lookup or generation for a fingerprint.
    /// Returns `true` if it was a cache hit (already observed).
    pub async fn record_access(&self, fingerprint: &str) -> bool {
        let mut map = self.fingerprints.write().await;
        if let Some(count) = map.get_mut(fingerprint) {
            *count += 1;
            true
        } else {
            map.insert(fingerprint.to_string(), 1);
            false
        }
    }

    /// Get total distinct cached prefixes.
    pub async fn cached_entries_count(&self) -> usize {
        let map = self.fingerprints.read().await;
        map.len()
    }
}
