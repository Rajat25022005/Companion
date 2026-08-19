use std::collections::HashMap;
use chrono::Utc;
use tokio::sync::RwLock;
use tracing::{info, warn};

use companion_domain::{RuntimeError, Skill, SkillLifecycleState};

/// Multi-version immutable registry for procedural skills (SkillOS).
pub struct SkillRegistry {
    /// Skill storage: skill_name -> list of immutable versions
    skills: RwLock<HashMap<String, Vec<Skill>>>,
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self {
            skills: RwLock::new(HashMap::new()),
        }
    }

    /// Register a new candidate skill version.
    /// Automatically increments version and sets state to Candidate.
    pub async fn register_candidate(&self, mut skill: Skill) -> Result<Skill, RuntimeError> {
        let key = skill.name.to_lowercase();
        let mut map = self.skills.write().await;
        let versions = map.entry(key.clone()).or_default();

        let next_version = (versions.len() as u32) + 1;
        skill.version = next_version;
        skill.lifecycle_state = SkillLifecycleState::Candidate;
        skill.provenance.parent_version = if next_version > 1 {
            Some(next_version - 1)
        } else {
            None
        };
        skill.updated_at = Utc::now();

        versions.push(skill.clone());
        info!(
            skill_name = %skill.name,
            version = skill.version,
            "registered new candidate skill version"
        );
        Ok(skill)
    }

    /// Register an already active / built-in skill (e.g. system seed skills).
    pub async fn register_active(&self, mut skill: Skill) {
        let key = skill.name.to_lowercase();
        let mut map = self.skills.write().await;
        let versions = map.entry(key).or_default();

        skill.lifecycle_state = SkillLifecycleState::Active;
        versions.push(skill);
    }

    /// Retrieve the currently active version of a skill.
    pub async fn get_active_skill(&self, name: &str) -> Option<Skill> {
        let key = name.to_lowercase();
        let map = self.skills.read().await;
        let versions = map.get(&key)?;

        // Look for Active, or fallback to Canary / Promoted
        versions
            .iter()
            .rev()
            .find(|s| matches!(s.lifecycle_state, SkillLifecycleState::Active | SkillLifecycleState::Promoted | SkillLifecycleState::Canary))
            .cloned()
    }

    /// Retrieve a specific version of a skill.
    pub async fn get_skill_version(&self, name: &str, version: u32) -> Option<Skill> {
        let key = name.to_lowercase();
        let map = self.skills.read().await;
        let versions = map.get(&key)?;

        versions.iter().find(|s| s.version == version).cloned()
    }

    /// List all currently active skills.
    pub async fn list_active_skills(&self) -> Vec<Skill> {
        let map = self.skills.read().await;
        let mut active = Vec::new();

        for versions in map.values() {
            if let Some(act) = versions.iter().rev().find(|s| s.lifecycle_state.is_executable()) {
                active.push(act.clone());
            }
        }

        active
    }

    /// List all versions of a skill.
    pub async fn list_versions(&self, name: &str) -> Vec<Skill> {
        let key = name.to_lowercase();
        let map = self.skills.read().await;
        map.get(&key).cloned().unwrap_or_default()
    }

    /// Stage a candidate skill for canary deployment.
    pub async fn stage_canary(&self, name: &str, version: u32) -> Result<Skill, RuntimeError> {
        let key = name.to_lowercase();
        let mut map = self.skills.write().await;
        let versions = map.get_mut(&key).ok_or_else(|| {
            RuntimeError::SkillError(format!("Skill `{name}` not found in registry"))
        })?;

        let target = versions
            .iter_mut()
            .find(|s| s.version == version)
            .ok_or_else(|| {
                RuntimeError::SkillError(format!("Skill `{name}` version {version} not found"))
            })?;

        target.lifecycle_state = SkillLifecycleState::Canary;
        target.updated_at = Utc::now();
        info!(skill_name = %name, version, "staged skill into CANARY mode");
        Ok(target.clone())
    }

    /// Promote a candidate / canary skill to active production.
    /// Deprecates previous active versions.
    pub async fn promote_skill(&self, name: &str, version: u32) -> Result<Skill, RuntimeError> {
        let key = name.to_lowercase();
        let mut map = self.skills.write().await;
        let versions = map.get_mut(&key).ok_or_else(|| {
            RuntimeError::SkillError(format!("Skill `{name}` not found in registry"))
        })?;

        // Deprecate previous active versions
        for v in versions.iter_mut() {
            if v.version != version && v.lifecycle_state == SkillLifecycleState::Active {
                v.lifecycle_state = SkillLifecycleState::Deprecated;
                v.updated_at = Utc::now();
            }
        }

        let target = versions
            .iter_mut()
            .find(|s| s.version == version)
            .ok_or_else(|| {
                RuntimeError::SkillError(format!("Skill `{name}` version {version} not found"))
            })?;

        target.lifecycle_state = SkillLifecycleState::Active;
        target.updated_at = Utc::now();
        info!(skill_name = %name, version, "promoted skill to ACTIVE production");
        Ok(target.clone())
    }

    /// Roll back an active or canary skill due to metric degradation or operator command.
    /// Reactivates the most recent previous stable version.
    pub async fn rollback_skill(&self, name: &str, reason: &str) -> Result<Skill, RuntimeError> {
        let key = name.to_lowercase();
        let mut map = self.skills.write().await;
        let versions = map.get_mut(&key).ok_or_else(|| {
            RuntimeError::SkillError(format!("Skill `{name}` not found in registry"))
        })?;

        let mut rolled_back_version = None;

        // Find current Active/Canary version and mark RolledBack
        for v in versions.iter_mut().rev() {
            if matches!(v.lifecycle_state, SkillLifecycleState::Active | SkillLifecycleState::Canary | SkillLifecycleState::Promoted) {
                v.lifecycle_state = SkillLifecycleState::RolledBack;
                v.updated_at = Utc::now();
                rolled_back_version = Some(v.version);
                warn!(skill_name = %name, version = v.version, reason = %reason, "rolled back skill version");
                break;
            }
        }

        // Find previous version to restore to Active
        let restore_target = versions
            .iter_mut()
            .rev()
            .find(|v| v.lifecycle_state == SkillLifecycleState::Deprecated || v.lifecycle_state == SkillLifecycleState::Staged);

        if let Some(target) = restore_target {
            target.lifecycle_state = SkillLifecycleState::Active;
            target.updated_at = Utc::now();
            info!(
                skill_name = %name,
                restored_version = target.version,
                "restored previous skill version to ACTIVE"
            );
            Ok(target.clone())
        } else if let Some(rb_v) = rolled_back_version {
            let rb_item = versions.iter().find(|v| v.version == rb_v).cloned().unwrap();
            Ok(rb_item)
        } else {
            Err(RuntimeError::SkillError(format!("No active version found to rollback for `{name}`")))
        }
    }

    /// Reject a candidate skill.
    pub async fn reject_skill(&self, name: &str, version: u32, reason: &str) -> Result<Skill, RuntimeError> {
        let key = name.to_lowercase();
        let mut map = self.skills.write().await;
        let versions = map.get_mut(&key).ok_or_else(|| {
            RuntimeError::SkillError(format!("Skill `{name}` not found in registry"))
        })?;

        let target = versions
            .iter_mut()
            .find(|s| s.version == version)
            .ok_or_else(|| {
                RuntimeError::SkillError(format!("Skill `{name}` version {version} not found"))
            })?;

        target.lifecycle_state = SkillLifecycleState::Rejected;
        target.updated_at = Utc::now();
        warn!(skill_name = %name, version, reason = %reason, "rejected candidate skill");
        Ok(target.clone())
    }

    /// Update runtime metrics for a specific skill version.
    pub async fn update_metrics(
        &self,
        name: &str,
        version: u32,
        success: bool,
        verified: bool,
        repaired: bool,
        tokens: u64,
        cost: f32,
    ) {
        let key = name.to_lowercase();
        let mut map = self.skills.write().await;
        if let Some(versions) = map.get_mut(&key) {
            if let Some(target) = versions.iter_mut().find(|s| s.version == version) {
                target.metrics.record_execution(success, verified, repaired, tokens, cost);
            }
        }
    }

    /// Match eligible active/canary skills for a task.
    pub async fn match_skills_for_task(&self, intent: &str, capabilities: &[String]) -> Vec<Skill> {
        let active_skills = self.list_active_skills().await;
        active_skills
            .into_iter()
            .filter(|s| s.matches_task(intent, capabilities))
            .collect()
    }
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}
