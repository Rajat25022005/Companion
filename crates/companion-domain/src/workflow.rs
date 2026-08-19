use std::collections::HashMap;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::cap::AgentRole;
use crate::ids::{AgentId, StepId, TaskId, WorkflowId};

// ---------------------------------------------------------------------------
// Step Retry Policy
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepRetryPolicy {
    pub max_retries: u32,
    pub backoff_secs: u64,
}

impl Default for StepRetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 2,
            backoff_secs: 2,
        }
    }
}

// ---------------------------------------------------------------------------
// Step Dependencies & Definition
// ---------------------------------------------------------------------------

/// Directed edge in the DAG (from -> to means `from` must complete before `to` can start).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StepDependency {
    pub from: StepId,
    pub to: StepId,
}

/// Definition of a single step inside a workflow DAG.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub step_id: StepId,
    pub name: String,
    pub description: String,
    pub assigned_role: AgentRole,
    pub prompt: String,
    #[serde(default)]
    pub required_tools: Vec<String>,
    #[serde(default)]
    pub retry_policy: StepRetryPolicy,
    #[serde(default = "default_step_timeout")]
    pub timeout_secs: u64,
}

fn default_step_timeout() -> u64 {
    300
}

/// Complete DAG workflow definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDef {
    pub workflow_id: WorkflowId,
    pub name: String,
    pub description: String,
    pub steps: Vec<WorkflowStep>,
    pub dependencies: Vec<StepDependency>,
}

impl WorkflowDef {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            workflow_id: WorkflowId::new(),
            name: name.into(),
            description: description.into(),
            steps: Vec::new(),
            dependencies: Vec::new(),
        }
    }

    pub fn add_step(&mut self, step: WorkflowStep) {
        self.steps.push(step);
    }

    pub fn add_dependency(&mut self, from: StepId, to: StepId) {
        self.dependencies.push(StepDependency { from, to });
    }
}

// ---------------------------------------------------------------------------
// Execution States
// ---------------------------------------------------------------------------

/// The runtime state of an individual step in the DAG.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum StepState {
    /// Step is waiting for dependencies to finish.
    Pending,
    /// All dependencies finished; ready to run.
    Ready,
    /// Actively running on an agent.
    Running {
        agent_id: AgentId,
        task_id: TaskId,
        started_at: DateTime<Utc>,
    },
    /// Step finished successfully.
    Completed {
        output: serde_json::Value,
        execution_ms: u64,
    },
    /// Step failed.
    Failed { reason: String },
    /// Step was skipped (e.g. due to upstream failure or conditional branch).
    Skipped,
}

impl StepState {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed { .. } | Self::Failed { .. } | Self::Skipped)
    }

    pub fn is_completed(&self) -> bool {
        matches!(self, Self::Completed { .. })
    }
}

/// Overall workflow lifecycle status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum WorkflowStatus {
    Created,
    Running,
    Completed,
    Failed { reason: String },
    Paused,
    Cancelled,
}

impl WorkflowStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed { .. } | Self::Cancelled)
    }
}

// ---------------------------------------------------------------------------
// Workflow State Snapshot (Checkpoint)
// ---------------------------------------------------------------------------

/// Materialized snapshot of a workflow's execution state for checkpointing and recovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStateSnapshot {
    pub workflow_id: WorkflowId,
    pub status: WorkflowStatus,
    pub step_states: HashMap<StepId, StepState>,
    pub step_outputs: HashMap<StepId, serde_json::Value>,
    pub sequence: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
