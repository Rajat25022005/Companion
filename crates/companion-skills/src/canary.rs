use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

use companion_domain::{RuntimeError, Skill, SkillLifecycleState};

use crate::registry::SkillRegistry;

/// Action decided by the canary controller based on observed runtime health metrics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanaryAction {
    ContinueCanary,
    Promoted,
    RolledBack,
}

/// Controller for managing canary deployments, traffic routing, and automated rollback.
pub struct CanaryController {
    registry: Arc<SkillRegistry>,
    canary_ratio: f32,
    min_evaluations_for_promotion: u64,
    max_failure_rate: f32,
    /// Canary execution counters: skill_name -> (successes, failures)
    canary_stats: RwLock<HashMap<String, (u64, u64)>>,
}

impl CanaryController {
    pub fn new(registry: Arc<SkillRegistry>) -> Self {
        Self {
            registry,
            canary_ratio: 0.20, // 20% traffic to canary by default
            min_evaluations_for_promotion: 5,
            max_failure_rate: 0.20,
            canary_stats: RwLock::new(HashMap::new()),
        }
    }

    pub fn with_canary_ratio(mut self, ratio: f32) -> Self {
        self.canary_ratio = ratio.clamp(0.0, 1.0);
        self
    }

    pub fn with_min_evaluations(mut self, min_evals: u64) -> Self {
        self.min_evaluations_for_promotion = min_evals;
        self
    }

    /// Select whether to route to the active version or canary version.
    pub async fn select_version(&self, name: &str, random_roll: f32) -> Option<Skill> {
        let versions = self.registry.list_versions(name).await;
        let canary = versions.iter().find(|v| v.lifecycle_state == SkillLifecycleState::Canary);
        let active = versions.iter().find(|v| v.lifecycle_state == SkillLifecycleState::Active);

        match (canary, active) {
            (Some(c), Some(a)) => {
                if random_roll <= self.canary_ratio {
                    Some(c.clone())
                } else {
                    Some(a.clone())
                }
            }
            (Some(c), None) => Some(c.clone()),
            (None, Some(a)) => Some(a.clone()),
            (None, None) => None,
        }
    }

    /// Record execution outcome for a canary skill and evaluate promotion/rollback rules.
    pub async fn record_canary_execution(
        &self,
        name: &str,
        canary_version: u32,
        success: bool,
    ) -> Result<CanaryAction, RuntimeError> {
        let key = name.to_lowercase();
        let (successes, failures) = {
            let mut map = self.canary_stats.write().await;
            let entry = map.entry(key.clone()).or_insert((0, 0));
            if success {
                entry.0 += 1;
            } else {
                entry.1 += 1;
            }
            *entry
        };

        let total = successes + failures;
        let failure_rate = failures as f32 / total as f32;

        // Check if rollback needed
        if total >= 2 && failure_rate > self.max_failure_rate {
            warn!(
                skill = %name,
                canary_version,
                failures,
                total,
                failure_rate,
                "canary failure rate exceeded tolerance -> auto-rollback"
            );
            self.registry.rollback_skill(name, "Canary metric degradation").await?;
            let mut map = self.canary_stats.write().await;
            map.remove(&key);
            return Ok(CanaryAction::RolledBack);
        }

        // Check if ready for promotion
        if total >= self.min_evaluations_for_promotion && failure_rate <= 0.10 {
            info!(
                skill = %name,
                canary_version,
                successes,
                total,
                "canary passed required test runs -> auto-promote to ACTIVE"
            );
            self.registry.promote_skill(name, canary_version).await?;
            let mut map = self.canary_stats.write().await;
            map.remove(&key);
            return Ok(CanaryAction::Promoted);
        }

        Ok(CanaryAction::ContinueCanary)
    }
}
