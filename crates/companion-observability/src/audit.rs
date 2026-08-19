use std::sync::Arc;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;
use tracing::info;

use companion_domain::{TaskId, TenantId};

/// An immutable, tamper-evident audit log entry in the cryptographic chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogEntry {
    pub sequence: u64,
    pub timestamp: DateTime<Utc>,
    pub tenant_id: TenantId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    pub actor: String,
    pub action: String,
    pub details: serde_json::Value,
    pub prev_hash: String,
    pub entry_hash: String,
}

impl AuditLogEntry {
    /// Compute the cryptographic SHA256 hash of this entry linked to prev_hash.
    pub fn compute_hash(
        sequence: u64,
        timestamp: &DateTime<Utc>,
        tenant_id: &TenantId,
        task_id: Option<&TaskId>,
        actor: &str,
        action: &str,
        details: &serde_json::Value,
        prev_hash: &str,
    ) -> String {
        let mut hasher = Sha256::new();
        hasher.update(prev_hash.as_bytes());
        hasher.update(sequence.to_be_bytes());
        hasher.update(timestamp.to_rfc3339().as_bytes());
        hasher.update(tenant_id.as_uuid().as_bytes());
        if let Some(t) = task_id {
            hasher.update(t.as_uuid().as_bytes());
        }
        hasher.update(actor.as_bytes());
        hasher.update(action.as_bytes());
        let details_str = serde_json::to_string(details).unwrap_or_default();
        hasher.update(details_str.as_bytes());

        format!("{:x}", hasher.finalize())
    }
}

/// Cryptographic append-only Audit Ledger ensuring tamper evidence for enterprise compliance.
#[derive(Clone)]
pub struct AuditLedger {
    entries: Arc<RwLock<Vec<AuditLogEntry>>>,
}

const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

impl AuditLedger {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Append a new security or operational action into the cryptographic hash chain.
    pub async fn append(
        &self,
        tenant_id: TenantId,
        task_id: Option<TaskId>,
        actor: &str,
        action: &str,
        details: serde_json::Value,
    ) -> AuditLogEntry {
        let mut list = self.entries.write().await;
        let sequence = (list.len() as u64) + 1;
        let timestamp = Utc::now();

        let prev_hash = if let Some(last) = list.last() {
            last.entry_hash.clone()
        } else {
            GENESIS_HASH.to_string()
        };

        let entry_hash = AuditLogEntry::compute_hash(
            sequence,
            &timestamp,
            &tenant_id,
            task_id.as_ref(),
            actor,
            action,
            &details,
            &prev_hash,
        );

        let entry = AuditLogEntry {
            sequence,
            timestamp,
            tenant_id,
            task_id,
            actor: actor.to_string(),
            action: action.to_string(),
            details,
            prev_hash,
            entry_hash: entry_hash.clone(),
        };

        list.push(entry.clone());
        info!(sequence, actor, action, entry_hash = %entry_hash, "recorded audit ledger entry");
        entry
    }

    /// List recent audit log entries.
    pub async fn get_entries(&self, limit: usize) -> Vec<AuditLogEntry> {
        let list = self.entries.read().await;
        list.iter().rev().take(limit).cloned().collect()
    }

    /// Total entries in the ledger.
    pub async fn count(&self) -> usize {
        self.entries.read().await.len()
    }

    /// Verify the cryptographic integrity of the entire audit hash chain.
    pub async fn verify_integrity(&self) -> Result<bool, String> {
        let list = self.entries.read().await;
        let mut expected_prev_hash = GENESIS_HASH.to_string();

        for (idx, entry) in list.iter().enumerate() {
            let seq = idx as u64 + 1;
            if entry.sequence != seq {
                return Err(format!(
                    "Sequence gap detected at index {idx}: expected {seq}, got {}",
                    entry.sequence
                ));
            }

            if entry.prev_hash != expected_prev_hash {
                return Err(format!(
                    "Hash chain broken at sequence {seq}: expected prev_hash `{expected_prev_hash}`, got `{}`",
                    entry.prev_hash
                ));
            }

            let computed = AuditLogEntry::compute_hash(
                entry.sequence,
                &entry.timestamp,
                &entry.tenant_id,
                entry.task_id.as_ref(),
                &entry.actor,
                &entry.action,
                &entry.details,
                &entry.prev_hash,
            );

            if computed != entry.entry_hash {
                return Err(format!(
                    "Tampering detected at sequence {seq}: computed `{computed}`, recorded `{}`",
                    entry.entry_hash
                ));
            }

            expected_prev_hash = entry.entry_hash.clone();
        }

        Ok(true)
    }
}

impl Default for AuditLedger {
    fn default() -> Self {
        Self::new()
    }
}
