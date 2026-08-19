use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::{CorrelationId, TaskId, TenantId, WorkspaceId};
use crate::intent::ModeProfile;

// ---------------------------------------------------------------------------
// Task Budget
// ---------------------------------------------------------------------------

/// Resource limits for a single task execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskBudget {
    /// Maximum model turns before forced failure.
    pub max_turns: u32,
    /// Maximum total tokens across all model calls.
    pub max_tokens: u64,
    /// Maximum wall-clock time in seconds.
    pub max_time_secs: u64,
    /// Maximum number of tool invocations.
    pub max_tool_calls: u32,
}

impl Default for TaskBudget {
    fn default() -> Self {
        Self {
            max_turns: 10,
            max_tokens: 100_000,
            max_time_secs: 300,
            max_tool_calls: 20,
        }
    }
}

// ---------------------------------------------------------------------------
// Completion Conditions
// ---------------------------------------------------------------------------

/// A deterministic condition that must be satisfied for task completion.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CompletionCondition {
    /// One or more files must exist at the given paths.
    FilesExist { paths: Vec<String> },

    /// A process must have exited with the given code (default 0).
    ProcessExitCode { command: String, expected_code: i32 },

    /// A specific capability must have been invoked at least once.
    ToolInvoked { capability: String },

    /// A model response must have been produced (for conversational tasks).
    ModelResponseProduced,

    /// Custom condition evaluated by a verifier function.
    Custom { name: String, params: serde_json::Value },
}

// ---------------------------------------------------------------------------
// Constraints
// ---------------------------------------------------------------------------

/// Execution constraints imposed on a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Constraint {
    /// Task must operate within this workspace directory.
    WorkspaceRoot { path: String },

    /// Task must not access network resources.
    NoNetwork,

    /// Task must not execute subprocesses.
    NoProcessExecution,

    /// Task must not modify filesystem (read only).
    ReadOnlyFilesystem,

    /// Custom constraint.
    Custom { name: String, params: serde_json::Value },
}

// ---------------------------------------------------------------------------
// Risk Level
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    /// No side effects (e.g., answering a question).
    None,
    /// Reads existing state (e.g., listing files).
    Low,
    /// Modifies workspace state (e.g., writing files).
    Medium,
    /// Executes arbitrary processes or modifies external systems.
    High,
    /// Deploys to production or modifies critical infrastructure.
    Critical,
}

// ---------------------------------------------------------------------------
// Capability Requirement
// ---------------------------------------------------------------------------

/// A capability that the task requires to be available.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityRequirement {
    /// The capability name (e.g., "filesystem.write").
    pub capability: String,
    /// Whether this capability is mandatory (must be invoked) or optional.
    pub required: bool,
}

// ---------------------------------------------------------------------------
// Task Contract
// ---------------------------------------------------------------------------

/// The compiled, runtime-enforceable specification of what a task must do.
///
/// Created by the ContractCompiler from a user request + intent classification.
/// The runtime uses this contract to enforce tool requirements, verify completion,
/// and bound resource usage. The LLM cannot modify the contract after compilation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskContract {
    pub task_id: TaskId,
    pub tenant_id: TenantId,
    pub workspace_id: WorkspaceId,
    pub correlation_id: CorrelationId,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_id: Option<crate::ids::WorkflowId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_id: Option<crate::ids::GoalId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_task_id: Option<TaskId>,

    /// The raw user input that produced this contract.
    pub user_input: String,

    /// The parsed objective / goal description.
    pub objective: String,

    /// The resolved mode profile (e.g., #build, #ask).
    pub mode_profile: ModeProfile,

    /// Capabilities that must be available and/or invoked.
    pub required_capabilities: Vec<CapabilityRequirement>,

    /// Tool names the agent is allowed to call.
    pub allowed_tools: Vec<String>,

    /// Conditions that must pass for the task to be marked COMPLETED.
    pub completion_conditions: Vec<CompletionCondition>,

    /// Constraints on execution.
    pub constraints: Vec<Constraint>,

    /// Risk assessment.
    pub risk_level: RiskLevel,

    /// Resource budget.
    pub budget: TaskBudget,

    /// When the contract was compiled.
    pub created_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Task State Machine
// ---------------------------------------------------------------------------

/// The authoritative state of a task. The runtime owns transitions.
/// An LLM cannot directly set task status to COMPLETED.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TaskState {
    /// Task has been created but not yet planned.
    Created,
    /// Task is being planned (capability resolution, context assembly).
    Planning,
    /// Task is ready to execute (plan approved, context compiled).
    Ready,
    /// Task is actively executing (model calls, tool calls in progress).
    Executing,
    /// Task is waiting for an asynchronous tool call to complete.
    WaitingTool { tool_call_id: String },
    /// Task is waiting for human or policy approval.
    WaitingApproval { approval_id: String },
    /// Task is verifying completion conditions against evidence.
    Verifying,
    /// Task completed successfully with all conditions met.
    Completed,
    /// Task failed with a reason.
    Failed { reason: String },
    /// Task is being repaired after a verification failure.
    Repairing { attempt: u32 },
    /// Task is suspended pending HITL dual-control approval.
    Suspended { reason: String, approval_id: String },
    /// Task is undergoing autonomous self-healing RCA and compensation.
    SelfHealing { attempt: u32, diagnosis: String },
    /// Task is paused (can be resumed).
    Paused,
    /// Task was cancelled.
    Cancelled,
}

impl TaskState {
    /// Returns the string label for this state (for display / storage).
    pub fn label(&self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Planning => "planning",
            Self::Ready => "ready",
            Self::Executing => "executing",
            Self::WaitingTool { .. } => "waiting_tool",
            Self::WaitingApproval { .. } => "waiting_approval",
            Self::Verifying => "verifying",
            Self::Completed => "completed",
            Self::Failed { .. } => "failed",
            Self::Repairing { .. } => "repairing",
            Self::Suspended { .. } => "suspended",
            Self::SelfHealing { .. } => "self_healing",
            Self::Paused => "paused",
            Self::Cancelled => "cancelled",
        }
    }

    /// Check whether transitioning to `next` is valid.
    pub fn can_transition_to(&self, next: &TaskState) -> bool {
        use TaskState::*;
        matches!(
            (self, next),
            // Forward flow
            (Created, Planning)
            | (Planning, Ready)
            | (Ready, Executing)
            | (Executing, WaitingTool { .. })
            | (Executing, WaitingApproval { .. })
            | (Executing, Verifying)
            | (Executing, Failed { .. })
            | (WaitingTool { .. }, Executing)
            | (WaitingApproval { .. }, Executing)
            | (Verifying, Completed)
            | (Verifying, Repairing { .. })
            | (Verifying, Failed { .. })
            | (Repairing { .. }, Executing)
            | (Repairing { .. }, Failed { .. })
            // HITL Suspension (Phase 10)
            | (Executing, Suspended { .. })
            | (Suspended { .. }, Executing)
            | (Suspended { .. }, Cancelled)
            // Self-Healing RCA (Phase 10)
            | (Failed { .. }, SelfHealing { .. })
            | (SelfHealing { .. }, Executing)
            | (SelfHealing { .. }, Failed { .. })
            // Pause / resume
            | (Executing, Paused)
            | (Paused, Executing)
            // Cancel from any active state
            | (Created, Cancelled)
            | (Planning, Cancelled)
            | (Ready, Cancelled)
            | (Executing, Cancelled)
            | (Paused, Cancelled)
            | (WaitingTool { .. }, Cancelled)
            | (WaitingApproval { .. }, Cancelled)
        )
    }

    /// Attempt a state transition. Returns the new state or an error.
    pub fn transition(self, next: TaskState) -> Result<TaskState, TransitionError> {
        if self.can_transition_to(&next) {
            Ok(next)
        } else {
            Err(TransitionError {
                from: self,
                to: next,
            })
        }
    }

    /// Whether this state is terminal (no further transitions expected).
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed { .. } | Self::Cancelled)
    }

    /// Whether this state is a suspended/blocked state.
    pub fn is_suspended(&self) -> bool {
        matches!(self, Self::Suspended { .. })
    }

    /// Whether this state is undergoing self-healing.
    pub fn is_self_healing(&self) -> bool {
        matches!(self, Self::SelfHealing { .. })
    }
}

impl std::fmt::Display for TaskState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// Error when an invalid state transition is attempted.
#[derive(Debug, Clone)]
pub struct TransitionError {
    pub from: TaskState,
    pub to: TaskState,
}

impl std::fmt::Display for TransitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "invalid state transition: {} → {}",
            self.from.label(),
            self.to.label()
        )
    }
}

impl std::error::Error for TransitionError {}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_forward_transitions() {
        let s = TaskState::Created;
        assert!(s.can_transition_to(&TaskState::Planning));

        let s = TaskState::Executing;
        assert!(s.can_transition_to(&TaskState::Verifying));
        assert!(s.can_transition_to(&TaskState::WaitingTool {
            tool_call_id: "t1".into()
        }));
    }

    #[test]
    fn test_invalid_transitions() {
        let s = TaskState::Created;
        assert!(!s.can_transition_to(&TaskState::Completed));
        assert!(!s.can_transition_to(&TaskState::Executing));

        let s = TaskState::Completed;
        assert!(!s.can_transition_to(&TaskState::Executing));
    }

    #[test]
    fn test_cancel_from_active_states() {
        for state in [
            TaskState::Created,
            TaskState::Planning,
            TaskState::Ready,
            TaskState::Executing,
            TaskState::Paused,
        ] {
            assert!(state.can_transition_to(&TaskState::Cancelled));
        }
    }

    #[test]
    fn test_terminal_states() {
        assert!(TaskState::Completed.is_terminal());
        assert!(TaskState::Failed {
            reason: "x".into()
        }
        .is_terminal());
        assert!(TaskState::Cancelled.is_terminal());
        assert!(!TaskState::Executing.is_terminal());
    }

    #[test]
    fn test_transition_returns_new_state() {
        let s = TaskState::Created;
        let next = s.transition(TaskState::Planning).unwrap();
        assert_eq!(next, TaskState::Planning);
    }

    #[test]
    fn test_transition_error() {
        let s = TaskState::Created;
        let err = s.transition(TaskState::Completed).unwrap_err();
        assert!(err.to_string().contains("invalid state transition"));
    }

    #[test]
    fn test_repair_loop() {
        let s = TaskState::Verifying;
        let s = s
            .transition(TaskState::Repairing { attempt: 1 })
            .unwrap();
        let s = s.transition(TaskState::Executing).unwrap();
        let s = s.transition(TaskState::Verifying).unwrap();
        let s = s.transition(TaskState::Completed).unwrap();
        assert_eq!(s, TaskState::Completed);
    }

    #[test]
    fn test_serde_roundtrip() {
        let state = TaskState::WaitingTool {
            tool_call_id: "abc".into(),
        };
        let json = serde_json::to_string(&state).unwrap();
        let restored: TaskState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, restored);
    }
}
