use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::{EvidenceId, GoalId, MilestoneId, TenantId, WorkflowId};

// ---------------------------------------------------------------------------
// Milestone
// ---------------------------------------------------------------------------

/// A concrete verifiable checkpoint towards achieving a Goal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Milestone {
    pub milestone_id: MilestoneId,
    pub title: String,
    pub description: String,
    pub completed: bool,
    #[serde(default)]
    pub evidence_ids: Vec<EvidenceId>,
    #[serde(default)]
    pub completed_at: Option<DateTime<Utc>>,
}

impl Milestone {
    pub fn new(title: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            milestone_id: MilestoneId::new(),
            title: title.into(),
            description: description.into(),
            completed: false,
            evidence_ids: Vec::new(),
            completed_at: None,
        }
    }

    pub fn complete(&mut self, evidence_ids: Vec<EvidenceId>) {
        self.completed = true;
        self.evidence_ids = evidence_ids;
        self.completed_at = Some(Utc::now());
    }
}

// ---------------------------------------------------------------------------
// Goal Status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum GoalStatus {
    Draft,
    Active,
    Suspended { reason: String },
    Completed,
    Failed { reason: String },
}

// ---------------------------------------------------------------------------
// Goal
// ---------------------------------------------------------------------------

/// A long-lived, high-level goal managed across multiple workflows and agent sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub goal_id: GoalId,
    pub tenant_id: TenantId,
    pub title: String,
    pub description: String,
    pub status: GoalStatus,
    pub milestones: Vec<Milestone>,
    pub active_workflow_id: Option<WorkflowId>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Goal {
    pub fn new(
        tenant_id: TenantId,
        title: impl Into<String>,
        description: impl Into<String>,
        milestones: Vec<Milestone>,
    ) -> Self {
        Self {
            goal_id: GoalId::new(),
            tenant_id,
            title: title.into(),
            description: description.into(),
            status: GoalStatus::Active,
            milestones,
            active_workflow_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    /// Check if all milestones are completed.
    pub fn is_all_milestones_completed(&self) -> bool {
        !self.milestones.is_empty() && self.milestones.iter().all(|m| m.completed)
    }

    /// Update milestone completion and auto-advance goal status if all done.
    pub fn mark_milestone_completed(&mut self, milestone_id: MilestoneId, evidence_ids: Vec<EvidenceId>) {
        if let Some(m) = self.milestones.iter_mut().find(|m| m.milestone_id == milestone_id) {
            m.complete(evidence_ids);
            self.updated_at = Utc::now();
        }

        if self.is_all_milestones_completed() {
            self.status = GoalStatus::Completed;
        }
    }
}
