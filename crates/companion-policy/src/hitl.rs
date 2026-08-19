use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{info, warn};

use companion_domain::{ApprovalId, RiskLevel, TaskId, TenantId};

// ---------------------------------------------------------------------------
// Approval Status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Approved {
        approver: String,
        approved_at: DateTime<Utc>,
    },
    Denied {
        reason: String,
        denied_at: DateTime<Utc>,
    },
    Expired,
}

// ---------------------------------------------------------------------------
// Approval Request
// ---------------------------------------------------------------------------

/// A request for human-in-the-loop dual-control approval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub approval_id: ApprovalId,
    pub task_id: TaskId,
    pub tenant_id: TenantId,
    pub risk_level: RiskLevel,
    pub action_description: String,
    pub requested_capabilities: Vec<String>,
    pub requested_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub status: ApprovalStatus,
}

// ---------------------------------------------------------------------------
// Policy Error
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum PolicyError {
    #[error("approval not found: {0}")]
    NotFound(String),

    #[error("approval already resolved: {0}")]
    AlreadyResolved(String),

    #[error("approval expired: {0}")]
    Expired(String),
}

// ---------------------------------------------------------------------------
// HITL Approval Gate
// ---------------------------------------------------------------------------

/// Manages the lifecycle of human-in-the-loop approval requests.
///
/// When a task encounters a high-risk operation that requires dual-control
/// sign-off, the runtime creates an `ApprovalRequest` through this gate.
/// The task transitions to `Suspended` until an operator approves or denies.
pub struct HitlApprovalGate {
    pending: RwLock<HashMap<ApprovalId, ApprovalRequest>>,
    default_timeout: Duration,
}

impl HitlApprovalGate {
    pub fn new(default_timeout: Duration) -> Self {
        Self {
            pending: RwLock::new(HashMap::new()),
            default_timeout,
        }
    }

    /// Create a new approval request and store it as pending.
    pub async fn request_approval(
        &self,
        task_id: TaskId,
        tenant_id: TenantId,
        risk_level: RiskLevel,
        description: String,
        capabilities: Vec<String>,
    ) -> ApprovalRequest {
        let approval_id = ApprovalId::new();
        let now = Utc::now();
        let expires_at = now + chrono::Duration::from_std(self.default_timeout).unwrap_or(chrono::Duration::hours(1));

        let request = ApprovalRequest {
            approval_id,
            task_id,
            tenant_id,
            risk_level,
            action_description: description,
            requested_capabilities: capabilities,
            requested_at: now,
            expires_at,
            status: ApprovalStatus::Pending,
        };

        info!(
            approval_id = %approval_id,
            task_id = %task_id,
            risk_level = ?risk_level,
            "HITL approval requested"
        );

        self.pending.write().await.insert(approval_id, request.clone());
        request
    }

    /// Approve a pending request. Returns error if not found or already resolved.
    pub async fn approve(
        &self,
        approval_id: ApprovalId,
        approver: String,
    ) -> Result<ApprovalRequest, PolicyError> {
        let mut pending = self.pending.write().await;
        let request = pending
            .get_mut(&approval_id)
            .ok_or_else(|| PolicyError::NotFound(approval_id.to_string()))?;

        if !matches!(request.status, ApprovalStatus::Pending) {
            return Err(PolicyError::AlreadyResolved(approval_id.to_string()));
        }

        if Utc::now() > request.expires_at {
            request.status = ApprovalStatus::Expired;
            return Err(PolicyError::Expired(approval_id.to_string()));
        }

        request.status = ApprovalStatus::Approved {
            approver: approver.clone(),
            approved_at: Utc::now(),
        };

        info!(
            approval_id = %approval_id,
            approver = %approver,
            "HITL approval GRANTED"
        );

        Ok(request.clone())
    }

    /// Deny a pending request.
    pub async fn deny(
        &self,
        approval_id: ApprovalId,
        reason: String,
    ) -> Result<ApprovalRequest, PolicyError> {
        let mut pending = self.pending.write().await;
        let request = pending
            .get_mut(&approval_id)
            .ok_or_else(|| PolicyError::NotFound(approval_id.to_string()))?;

        if !matches!(request.status, ApprovalStatus::Pending) {
            return Err(PolicyError::AlreadyResolved(approval_id.to_string()));
        }

        request.status = ApprovalStatus::Denied {
            reason: reason.clone(),
            denied_at: Utc::now(),
        };

        warn!(
            approval_id = %approval_id,
            reason = %reason,
            "HITL approval DENIED"
        );

        Ok(request.clone())
    }

    /// List all pending approval requests for a tenant.
    pub async fn list_pending(&self, tenant_id: Option<TenantId>) -> Vec<ApprovalRequest> {
        let pending = self.pending.read().await;
        pending
            .values()
            .filter(|r| matches!(r.status, ApprovalStatus::Pending))
            .filter(|r| tenant_id.is_none() || Some(r.tenant_id) == tenant_id)
            .cloned()
            .collect()
    }

    /// Sweep and expire timed-out pending approvals. Returns count of expired.
    pub async fn check_expiry(&self) -> usize {
        let now = Utc::now();
        let mut pending = self.pending.write().await;
        let mut expired_count = 0;

        for request in pending.values_mut() {
            if matches!(request.status, ApprovalStatus::Pending) && now > request.expires_at {
                request.status = ApprovalStatus::Expired;
                expired_count += 1;
                warn!(
                    approval_id = %request.approval_id,
                    task_id = %request.task_id,
                    "HITL approval EXPIRED"
                );
            }
        }

        expired_count
    }

    /// Get a specific approval request by ID.
    pub async fn get(&self, approval_id: ApprovalId) -> Option<ApprovalRequest> {
        self.pending.read().await.get(&approval_id).cloned()
    }

    /// Total count of all requests (any status).
    pub async fn total_count(&self) -> usize {
        self.pending.read().await.len()
    }
}
