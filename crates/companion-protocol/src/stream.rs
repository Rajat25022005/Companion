use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Server-Sent Events (SSE) stream event payload in the Companion Runtime Protocol (CRP).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type", content = "data", rename_all = "snake_case")]
pub enum TaskStreamEvent {
    /// Task has been created and planning has started.
    TaskCreated {
        task_id: Uuid,
        objective: String,
        created_at: DateTime<Utc>,
    },
    /// A new execution turn has started.
    TurnStarted {
        task_id: Uuid,
        turn: u32,
    },
    /// Model has proposed a tool invocation.
    ToolCallProposed {
        task_id: Uuid,
        tool: String,
        arguments: serde_json::Value,
    },
    /// Tool execution has completed with evidence output.
    ToolCallExecuted {
        task_id: Uuid,
        tool: String,
        success: bool,
        output: serde_json::Value,
        content_hash: Option<String>,
        duration_ms: u64,
    },
    /// Turn finished with assistant response or repair instruction.
    TurnCompleted {
        task_id: Uuid,
        turn: u32,
        content: String,
    },
    /// Task successfully reached terminal Completed state.
    TaskCompleted {
        task_id: Uuid,
        completed_at: DateTime<Utc>,
    },
    /// Task failed with reason.
    TaskFailed {
        task_id: Uuid,
        reason: String,
        failed_at: DateTime<Utc>,
    },
    /// Keep-alive heartbeat for long-running streaming connections.
    Heartbeat {
        timestamp: DateTime<Utc>,
    },
}

impl TaskStreamEvent {
    /// Serialize this stream event into standard Server-Sent Events (SSE) format.
    pub fn to_sse_message(&self) -> String {
        let json = serde_json::to_string(self).unwrap_or_default();
        let event_name = match self {
            Self::TaskCreated { .. } => "task_created",
            Self::TurnStarted { .. } => "turn_started",
            Self::ToolCallProposed { .. } => "tool_proposed",
            Self::ToolCallExecuted { .. } => "tool_executed",
            Self::TurnCompleted { .. } => "turn_completed",
            Self::TaskCompleted { .. } => "task_completed",
            Self::TaskFailed { .. } => "task_failed",
            Self::Heartbeat { .. } => "heartbeat",
        };

        format!("event: {event_name}\ndata: {json}\n\n")
    }
}
