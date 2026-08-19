use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Macro to generate a strongly-typed UUID newtype.
///
/// Each ID type is Copy, Clone, Eq, Hash, and serializes as a UUID string.
macro_rules! define_id {
    ($name:ident, $doc:expr) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Create a new time-ordered ID (UUIDv7).
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            /// Create from an existing UUID.
            pub fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            /// Get the inner UUID.
            pub fn as_uuid(&self) -> &Uuid {
                &self.0
            }

            /// Create a nil/zero ID (useful for tests).
            pub fn nil() -> Self {
                Self(Uuid::nil())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                // Short display: first 8 chars of the UUID
                write!(f, "{}", &self.0.to_string()[..8])
            }
        }

        impl From<Uuid> for $name {
            fn from(uuid: Uuid) -> Self {
                Self(uuid)
            }
        }

        impl From<$name> for Uuid {
            fn from(id: $name) -> Uuid {
                id.0
            }
        }
    };
}

define_id!(TaskId, "Unique identifier for a task.");
define_id!(TenantId, "Unique identifier for a tenant / organization.");
define_id!(WorkspaceId, "Unique identifier for a workspace / project.");
define_id!(AgentId, "Unique identifier for an agent instance.");
define_id!(EventId, "Unique identifier for a task event.");
define_id!(EvidenceId, "Unique identifier for an evidence record.");
define_id!(ArtifactId, "Unique identifier for an artifact.");
define_id!(CorrelationId, "Correlation ID for tracing request chains.");
define_id!(CapabilityId, "Unique identifier for a registered capability.");
define_id!(CheckpointId, "Unique identifier for an execution checkpoint.");
define_id!(CapMessageId, "Unique identifier for a CAP message.");
define_id!(ConversationId, "Unique identifier for an inter-agent conversation thread.");
define_id!(WorkflowId, "Unique identifier for a DAG workflow.");
define_id!(StepId, "Unique identifier for a step within a workflow DAG.");
define_id!(GoalId, "Unique identifier for a long-lived goal.");
define_id!(MilestoneId, "Unique identifier for a goal milestone.");
define_id!(MemoryId, "Unique identifier for a memory item.");
define_id!(EntityId, "Unique identifier for a knowledge graph entity.");
define_id!(RelationshipId, "Unique identifier for a knowledge graph relation.");
define_id!(GrantId, "Unique identifier for a context grant.");
define_id!(ContextId, "Unique identifier for a compiled context instance.");
define_id!(SessionId, "Unique identifier for a conversation/agent session.");
define_id!(SkillId, "Unique identifier for a procedural skill.");
define_id!(ApprovalId, "Unique identifier for a HITL approval request.");
define_id!(PolicyRuleId, "Unique identifier for a declarative policy rule.");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ids_are_unique() {
        let a = TaskId::new();
        let b = TaskId::new();
        assert_ne!(a, b);
    }

    #[test]
    fn test_id_display_is_short() {
        let id = TaskId::new();
        let display = format!("{id}");
        assert_eq!(display.len(), 8);
    }

    #[test]
    fn test_id_roundtrip_serde() {
        let id = TaskId::new();
        let json = serde_json::to_string(&id).unwrap();
        let restored: TaskId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, restored);
    }

    #[test]
    fn test_nil_id() {
        let id = TaskId::nil();
        assert_eq!(id.as_uuid(), &Uuid::nil());
    }
}
