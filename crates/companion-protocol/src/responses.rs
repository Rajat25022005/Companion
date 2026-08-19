use std::collections::HashMap;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use companion_domain::{TaskContract, TaskState};

/// Response for a created task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResponse {
    pub task_id: Uuid,
    pub state: TaskState,
    pub created_at: DateTime<Utc>,
}

/// Response for task detail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDetailResponse {
    pub task_id: Uuid,
    pub state: TaskState,
    pub contract: TaskContract,
    pub created_at: DateTime<Utc>,
}

/// Health check response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub providers: HashMap<String, bool>,
}

/// Readiness check response for Kubernetes/container orchestration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadyResponse {
    pub ready: bool,
    pub storage_connected: bool,
    pub default_provider_ready: bool,
    pub uptime_seconds: u64,
}

/// Response containing skill summary information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillSummaryResponse {
    pub name: String,
    pub active_version: String,
    pub total_versions: usize,
    pub description: String,
    pub state: String,
}

/// Response for skill operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillOperationResponse {
    pub success: bool,
    pub message: String,
    pub current_version: String,
}

/// Response for cryptographic audit ledger verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditVerificationResponse {
    pub intact: bool,
    pub total_entries: usize,
    pub message: String,
}

/// Error response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
    pub code: String,
}
