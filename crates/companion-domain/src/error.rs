/// Unified error types for the Companion runtime.
///
/// Domain errors use `thiserror` for explicit error variants.
/// Application-level code can use `anyhow` for ad-hoc error propagation.

// ---------------------------------------------------------------------------
// Runtime Error
// ---------------------------------------------------------------------------

/// Top-level error type for the Companion runtime.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("task failed: {0}")]
    TaskFailed(String),

    #[error("invalid state transition: {from} → {to}")]
    InvalidTransition { from: String, to: String },

    #[error("task not found: {0}")]
    TaskNotFound(String),

    #[error("budget exceeded: {resource} (limit: {limit}, used: {used})")]
    BudgetExceeded {
        resource: String,
        limit: u64,
        used: u64,
    },

    #[error("verification failed: {0}")]
    VerificationFailed(String),

    #[error("capability not found: {0}")]
    CapabilityNotFound(String),

    #[error("authorization denied: {0}")]
    AuthorizationDenied(String),

    #[error("contract compilation error: {0}")]
    ContractCompilationError(String),

    #[error("model error: {0}")]
    ModelError(#[from] crate::model::ModelError),

    #[error("storage error: {0}")]
    StorageError(String),

    #[error("skill error: {0}")]
    SkillError(String),

    #[error("policy violation: {0}")]
    PolicyViolation(String),

    #[error("approval required: {reason} (approval_id: {approval_id})")]
    ApprovalRequired { approval_id: String, reason: String },

    #[error("internal error: {0}")]
    Internal(String),
}

// ---------------------------------------------------------------------------
// Event Store Error
// ---------------------------------------------------------------------------

/// Errors from event store operations.
#[derive(Debug, thiserror::Error)]
pub enum EventStoreError {
    #[error("failed to append event: {0}")]
    AppendFailed(String),

    #[error("failed to load events: {0}")]
    LoadFailed(String),

    #[error("sequence conflict: expected {expected}, got {actual}")]
    SequenceConflict { expected: i64, actual: i64 },

    #[error("connection error: {0}")]
    ConnectionError(String),

    #[error("serialization error: {0}")]
    SerializationError(String),
}

// ---------------------------------------------------------------------------
// Task Store Error
// ---------------------------------------------------------------------------

/// Errors from task store operations.
#[derive(Debug, thiserror::Error)]
pub enum TaskStoreError {
    #[error("task not found: {0}")]
    NotFound(String),

    #[error("failed to save task: {0}")]
    SaveFailed(String),

    #[error("connection error: {0}")]
    ConnectionError(String),
}
