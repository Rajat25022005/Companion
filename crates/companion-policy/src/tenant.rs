use std::path::{Path, PathBuf};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use companion_domain::{RuntimeError, TenantId, WorkspaceId};

/// Claims encoded inside a tenant-scoped authorization token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantAuthClaims {
    pub tenant_id: TenantId,
    pub workspace_id: WorkspaceId,
    pub roles: Vec<String>,
    pub expires_at: DateTime<Utc>,
}

/// Tenant security manager enforcing multi-tenant isolation and token validation.
#[derive(Clone)]
pub struct TenantSecurityManager {
    base_storage_dir: PathBuf,
    secret_key: String,
}

impl TenantSecurityManager {
    pub fn new(base_storage_dir: impl Into<PathBuf>, secret_key: impl Into<String>) -> Self {
        Self {
            base_storage_dir: base_storage_dir.into(),
            secret_key: secret_key.into(),
        }
    }

    /// Resolve and validate the tenant's isolated workspace directory.
    pub fn resolve_tenant_workspace(
        &self,
        tenant_id: &TenantId,
        workspace_id: &WorkspaceId,
    ) -> PathBuf {
        self.base_storage_dir
            .join(tenant_id.to_string())
            .join(workspace_id.to_string())
    }

    /// Enforce that a requested target path stays strictly inside the tenant workspace root.
    pub fn validate_path_isolation(
        &self,
        tenant_id: &TenantId,
        workspace_id: &WorkspaceId,
        target_path: &Path,
    ) -> Result<PathBuf, RuntimeError> {
        let workspace_root = self.resolve_tenant_workspace(tenant_id, workspace_id);

        // Normalize / resolve relative path components
        let target_str = target_path.to_string_lossy();
        if target_str.contains("..") {
            return Err(RuntimeError::AuthorizationDenied(format!(
                "Path traversal attempt rejected for path: {target_str}"
            )));
        }

        let full_path = if target_path.is_absolute() {
            target_path.to_path_buf()
        } else {
            workspace_root.join(target_path)
        };

        if !full_path.starts_with(&workspace_root) && target_path.is_absolute() {
            return Err(RuntimeError::AuthorizationDenied(format!(
                "Cross-tenant access violation: path `{}` is outside tenant workspace `{}`",
                full_path.display(),
                workspace_root.display()
            )));
        }

        Ok(full_path)
    }

    /// Sign and generate a tenant authorization token string.
    pub fn issue_token(&self, claims: &TenantAuthClaims) -> String {
        let claims_json = serde_json::to_string(claims).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(claims_json.as_bytes());
        hasher.update(self.secret_key.as_bytes());
        let signature = format!("{:x}", hasher.finalize());

        let payload = serde_json::json!({
            "claims": claims,
            "sig": signature
        });

        serde_json::to_string(&payload).unwrap_or_default()
    }

    /// Validate a tenant authorization token and extract claims.
    pub fn validate_token(&self, token_str: &str) -> Result<TenantAuthClaims, RuntimeError> {
        let val: serde_json::Value = serde_json::from_str(token_str).map_err(|e| {
            RuntimeError::AuthorizationDenied(format!("Invalid token JSON structure: {e}"))
        })?;

        let claims_val = val.get("claims").ok_or_else(|| {
            RuntimeError::AuthorizationDenied("Token missing claims payload".into())
        })?;

        let sig = val.get("sig").and_then(|s| s.as_str()).ok_or_else(|| {
            RuntimeError::AuthorizationDenied("Token missing signature".into())
        })?;

        let claims: TenantAuthClaims = serde_json::from_value(claims_val.clone()).map_err(|e| {
            RuntimeError::AuthorizationDenied(format!("Invalid claims: {e}"))
        })?;

        // Verify signature
        let claims_json = serde_json::to_string(&claims).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(claims_json.as_bytes());
        hasher.update(self.secret_key.as_bytes());
        let expected_sig = format!("{:x}", hasher.finalize());

        if sig != expected_sig {
            return Err(RuntimeError::AuthorizationDenied(
                "Invalid token cryptographic signature".into(),
            ));
        }

        // Check expiration
        if Utc::now() > claims.expires_at {
            return Err(RuntimeError::AuthorizationDenied(
                "Tenant authorization token has expired".into(),
            ));
        }

        Ok(claims)
    }
}
