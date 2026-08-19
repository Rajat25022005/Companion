use std::collections::HashMap;
use chrono::Utc;
use tokio::sync::RwLock;
use companion_domain::{MemoryFilter, MemoryId, MemoryItem, MemorySearchResult, MemoryStatus, MemoryTier};

/// In-memory vector store with Cosine Similarity, Recency Decay, Trust Weighting, and Multi-Factor Ranking.
pub struct VectorStore {
    items: RwLock<HashMap<MemoryId, MemoryItem>>,
}

impl VectorStore {
    pub fn new() -> Self {
        Self {
            items: RwLock::new(HashMap::new()),
        }
    }

    /// Insert or update a memory item.
    pub async fn insert(&self, item: MemoryItem) {
        let mut map = self.items.write().await;
        map.insert(item.memory_id, item);
    }

    /// Retrieve an item by its ID.
    pub async fn get(&self, memory_id: &MemoryId) -> Option<MemoryItem> {
        let map = self.items.read().await;
        map.get(memory_id).cloned()
    }

    /// Mark an existing memory item as superseded by a newer one.
    pub async fn supersede(&self, old_id: &MemoryId, new_id: MemoryId) -> bool {
        let mut map = self.items.write().await;
        if let Some(item) = map.get_mut(old_id) {
            item.supersede_with(new_id);
            true
        } else {
            false
        }
    }

    /// Touch an item to update its accessed timestamp and access count.
    pub async fn touch(&self, memory_id: &MemoryId) {
        let mut map = self.items.write().await;
        if let Some(item) = map.get_mut(memory_id) {
            item.touch();
        }
    }

    /// Remove a memory item permanently.
    pub async fn remove(&self, memory_id: &MemoryId) -> Option<MemoryItem> {
        let mut map = self.items.write().await;
        map.remove(memory_id)
    }

    /// Query nearest memory items using cosine similarity with legacy signature.
    pub async fn search(
        &self,
        query_vector: &[f32],
        limit: usize,
        min_similarity: f32,
        tier_filter: Option<MemoryTier>,
    ) -> Vec<MemorySearchResult> {
        let filter = MemoryFilter {
            tier: tier_filter,
            active_only: true,
            ..Default::default()
        };
        self.search_with_filter(query_vector, limit, min_similarity, &filter).await
    }

    /// Multi-factor search combining Cosine Similarity, Importance, Trust Class, and Recency Decay.
    ///
    /// Formula:
    /// Score = (sim * 0.45) + (norm_importance * 0.20) + (trust_weight * 0.20) + (recency_decay * 0.15)
    pub async fn search_with_filter(
        &self,
        query_vector: &[f32],
        limit: usize,
        min_similarity: f32,
        filter: &MemoryFilter,
    ) -> Vec<MemorySearchResult> {
        let now = Utc::now();
        let map = self.items.read().await;
        let mut scored = Vec::new();

        for item in map.values() {
            // Apply filtering
            if filter.active_only && item.status != MemoryStatus::Active {
                continue;
            }
            if !item.is_valid_at(now) {
                continue;
            }
            if let Some(t) = filter.tier {
                if item.tier != t {
                    continue;
                }
            }
            if let Some(tenant) = filter.tenant_id {
                if item.tenant_id != Some(tenant) {
                    continue;
                }
            }
            if let Some(ws) = filter.workspace_id {
                if item.workspace_id != Some(ws) {
                    continue;
                }
            }
            if let Some(min_t) = filter.min_trust {
                if item.trust_class.authority_rank() < min_t.authority_rank() {
                    continue;
                }
            }
            if let Some(min_imp) = filter.min_importance {
                if item.importance < min_imp {
                    continue;
                }
            }
            if let Some(ref subj) = filter.subject {
                if item.subject.as_ref() != Some(subj) {
                    continue;
                }
            }

            if let Some(ref emb) = item.embedding {
                let sim = cosine_similarity(query_vector, emb);
                if sim >= min_similarity {
                    // Multi-factor components
                    let norm_sim = sim.max(0.0);
                    let norm_imp = (item.importance / 5.0).clamp(0.0, 1.0);
                    let trust_val = item.trust_class.trust_weight();
                    let decay_val = item.recency_decay(now, 30.0); // 30-day half-life

                    let total_score = (norm_sim * 0.45)
                        + (norm_imp * 0.20)
                        + (trust_val * 0.20)
                        + (decay_val * 0.15);

                    let mut match_reasons = Vec::new();
                    match_reasons.push(format!("similarity={:.2}", sim));
                    match_reasons.push(format!("trust={:?}", item.trust_class));
                    match_reasons.push(format!("importance={:.1}", item.importance));

                    scored.push(MemorySearchResult {
                        item: item.clone(),
                        score: total_score,
                        tier: item.tier,
                        match_reasons,
                    });
                }
            }
        }

        // Sort descending by score
        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        scored
    }

    /// List all active memory items.
    pub async fn list_all(&self) -> Vec<MemoryItem> {
        let map = self.items.read().await;
        map.values().cloned().collect()
    }

    /// Get total number of items stored.
    pub async fn count(&self) -> usize {
        let map = self.items.read().await;
        map.len()
    }
}

impl Default for VectorStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute cosine similarity between two float slices.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || b.is_empty() || a.len() != b.len() {
        return 0.0;
    }

    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;

    for i in 0..a.len() {
        dot += a[i] * b[i];
        norm_a += a[i] * a[i];
        norm_b += b[i] * b[i];
    }

    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 1e-5);

        let c = vec![0.0, 1.0, 0.0];
        assert!((cosine_similarity(&a, &c) - 0.0).abs() < 1e-5);
    }
}
