use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use companion_domain::{CorrelationId, EventId, TaskId, TaskState};

/// A durable event in the task lifecycle.
///
/// Every state transition, model call, tool call, and verification step
/// is recorded as an event. Events are the source of truth for task history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEvent {
    pub event_id: EventId,
    pub task_id: TaskId,
    pub correlation_id: CorrelationId,
    pub timestamp: DateTime<Utc>,
    /// Monotonically increasing sequence number within a task.
    pub sequence: i64,
    pub event_type: TaskEventType,
    /// Structured payload specific to the event type.
    pub payload: serde_json::Value,
}

impl TaskEvent {
    /// Create a new event with auto-generated ID and timestamp.
    pub fn new(
        task_id: TaskId,
        correlation_id: CorrelationId,
        sequence: i64,
        event_type: TaskEventType,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            event_id: EventId::new(),
            task_id,
            correlation_id,
            timestamp: Utc::now(),
            sequence,
            event_type,
            payload,
        }
    }
}

/// The type of task event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskEventType {
    /// Task was created from user input.
    TaskCreated,
    /// TaskContract was compiled from intent.
    TaskContractCompiled,
    /// Task transitioned between states.
    StateTransition,
    /// A model call was initiated.
    ModelCallStarted,
    /// A model call completed.
    ModelCallCompleted,
    /// A tool call was requested by the model.
    ToolCallRequested,
    /// A tool call began execution.
    ToolCallStarted,
    /// A tool call completed successfully.
    ToolCallCompleted,
    /// A tool call failed.
    ToolCallFailed,
    /// Evidence was collected from tool results.
    EvidenceCollected,
    /// Verification of completion conditions started.
    VerificationStarted,
    /// All completion conditions passed.
    VerificationPassed,
    /// One or more completion conditions failed.
    VerificationFailed,
    /// Task completed successfully.
    TaskCompleted,
    /// Task failed.
    TaskFailed,
    /// Execution checkpoint created.
    CheckpointCreated,
    /// Turn rejected by Tool Intent Monitor.
    TurnRejected,
    /// Authorization decision was made.
    AuthorizationDecision,
    /// Human-in-the-loop approval was requested.
    ApprovalRequested,
    /// Human-in-the-loop approval was resolved (approved/denied).
    ApprovalResolved,
    /// Autonomous self-healing was attempted.
    SelfHealingAttempted,
}

impl std::fmt::Display for TaskEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = serde_json::to_value(self)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| format!("{self:?}"));
        write!(f, "{s}")
    }
}

/// Payload for a state transition event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateTransitionPayload {
    pub from: TaskState,
    pub to: TaskState,
    pub reason: Option<String>,
}

/// Payload for a model call completed event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCallPayload {
    pub model: String,
    pub provider: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub latency_ms: u64,
    pub has_tool_calls: bool,
    pub finish_reason: String,
}

/// Payload for a tool call event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallPayload {
    pub tool_call_id: String,
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub result: Option<serde_json::Value>,
    pub success: Option<bool>,
    pub execution_ms: Option<u64>,
    pub error: Option<String>,
}
