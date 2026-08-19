use serde::{Deserialize, Serialize};

/// Request to create and execute a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTaskRequest {
    /// The user's input (e.g., "#build Create hello.txt with Hello World").
    pub input: String,

    /// Optional model override.
    pub model: Option<String>,

    /// Optional workspace root path.
    pub workspace: Option<String>,
}

/// Request to promote a skill version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromoteSkillRequest {
    pub skill_name: String,
    pub target_version: String,
    pub reason: Option<String>,
}

/// Request to rollback a skill to a previous stable version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackSkillRequest {
    pub skill_name: String,
    pub reason: String,
}

/// Request to verify audit ledger integrity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyAuditRequest {
    pub max_entries: Option<usize>,
}
