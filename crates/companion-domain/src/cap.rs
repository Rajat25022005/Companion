use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::{AgentId, ArtifactId, CapMessageId, ConversationId, CorrelationId};
use crate::task::TaskContract;

// ---------------------------------------------------------------------------
// Agent Role & Address
// ---------------------------------------------------------------------------

/// Well-known agent specializations / roles in Companion.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    /// Orchestrator / Decomposition & Team Leader.
    Coordinator,
    /// System designer, technical planner & interface author.
    Architect,
    /// Code implementer & tool executor.
    Engineer,
    /// Quality assurance, security auditor & verifier.
    Reviewer,
    /// Information gatherer, documentation explorer & web scraper.
    Researcher,
    /// Custom role identifier.
    Custom(String),
}

impl std::fmt::Display for AgentRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Coordinator => write!(f, "coordinator"),
            Self::Architect => write!(f, "architect"),
            Self::Engineer => write!(f, "engineer"),
            Self::Reviewer => write!(f, "reviewer"),
            Self::Researcher => write!(f, "researcher"),
            Self::Custom(name) => write!(f, "custom({name})"),
        }
    }
}

/// Identifies an agent participant in a CAP interaction.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentAddress {
    pub agent_id: AgentId,
    pub role: AgentRole,
}

impl AgentAddress {
    pub fn new(role: AgentRole) -> Self {
        Self {
            agent_id: AgentId::new(),
            role,
        }
    }

    pub fn with_id(agent_id: AgentId, role: AgentRole) -> Self {
        Self { agent_id, role }
    }
}

// ---------------------------------------------------------------------------
// Artifact References
// ---------------------------------------------------------------------------

/// A lightweight reference to a durable artifact produced by an agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactReference {
    pub artifact_id: ArtifactId,
    pub name: String,
    pub content_hash: Option<String>,
    pub uri: String,
    pub mime_type: Option<String>,
}

// ---------------------------------------------------------------------------
// CAP Payloads
// ---------------------------------------------------------------------------

/// Typed message payload transmitted between agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CapPayload {
    /// General query or message text.
    Text { content: String },

    /// Architectural plan or task specification.
    PlanSpecification {
        title: String,
        summary: String,
        subtasks: Vec<String>,
        artifacts: Vec<ArtifactReference>,
    },

    /// Delegation request containing a compiled TaskContract.
    TaskDelegation {
        contract: Box<TaskContract>,
        priority: u32,
    },

    /// Result or output from an executed task.
    TaskResult {
        success: bool,
        output: serde_json::Value,
        evidence_summary: Option<String>,
    },

    /// Review verdict on artifacts or task outputs.
    ReviewVerdict {
        approved: bool,
        feedback: String,
        suggested_revisions: Vec<String>,
    },

    /// Error notification.
    Error {
        code: String,
        message: String,
        retryable: bool,
    },

    /// Arbitrary structured data.
    Json { data: serde_json::Value },
}

// ---------------------------------------------------------------------------
// Message Patterns
// ---------------------------------------------------------------------------

/// Interaction pattern for a CAP message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "pattern", rename_all = "snake_case")]
pub enum MessagePattern {
    /// Direct request expecting a paired response.
    Request,

    /// Response correlated to an earlier request.
    Response { in_reply_to: CapMessageId },

    /// Explicit delegation of control and responsibility.
    Delegate,

    /// State and context handoff without waiting for return.
    Handoff,

    /// Topic-based pub/sub broadcast.
    Broadcast { topic: String },

    /// One-way notification or telemetry event.
    Event { name: String },
}

// ---------------------------------------------------------------------------
// CAP Envelope
// ---------------------------------------------------------------------------

/// The fundamental message unit of the Companion Agent Protocol (CAP).
///
/// Encapsulates sender, recipient, routing, correlation, references, and typed payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapEnvelope {
    pub message_id: CapMessageId,
    pub correlation_id: CorrelationId,
    pub conversation_id: ConversationId,
    pub sender: AgentAddress,
    pub recipient: AgentAddress,
    pub pattern: MessagePattern,
    pub payload: CapPayload,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<ArtifactReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl_secs: Option<u64>,
    pub timestamp: DateTime<Utc>,
}

impl CapEnvelope {
    pub fn new(
        sender: AgentAddress,
        recipient: AgentAddress,
        correlation_id: CorrelationId,
        conversation_id: ConversationId,
        pattern: MessagePattern,
        payload: CapPayload,
    ) -> Self {
        Self {
            message_id: CapMessageId::new(),
            correlation_id,
            conversation_id,
            sender,
            recipient,
            pattern,
            payload,
            references: Vec::new(),
            ttl_secs: Some(300),
            timestamp: Utc::now(),
        }
    }

    /// Create a response envelope to this message.
    pub fn create_response(&self, sender: AgentAddress, payload: CapPayload) -> Self {
        Self {
            message_id: CapMessageId::new(),
            correlation_id: self.correlation_id,
            conversation_id: self.conversation_id,
            sender,
            recipient: self.sender.clone(),
            pattern: MessagePattern::Response {
                in_reply_to: self.message_id,
            },
            payload,
            references: Vec::new(),
            ttl_secs: self.ttl_secs,
            timestamp: Utc::now(),
        }
    }
}
