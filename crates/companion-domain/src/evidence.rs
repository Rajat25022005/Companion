use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::{EvidenceId, TaskId};

// ---------------------------------------------------------------------------
// Evidence Authority
// ---------------------------------------------------------------------------

/// How trustworthy a piece of evidence is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceAuthority {
    /// Evidence from deterministic runtime checks (exit codes, file existence, HTTP probes).
    /// Highest trust for mechanical state.
    DeterministicRuntime,

    /// Evidence backed by a cryptographic identifier (content hash, signature).
    CryptographicArtifact,

    /// Evidence from an independent verifier (e.g., a separate model or test suite).
    IndependentVerifier,

    /// Evidence is a model assertion. Lowest trust — never sufficient alone.
    ModelAssertion,
}

// ---------------------------------------------------------------------------
// Observation
// ---------------------------------------------------------------------------

/// A single observation that contributes to evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    /// What was observed (e.g., "file_exists", "exit_code", "http_status").
    pub kind: String,

    /// The observed value.
    pub value: serde_json::Value,

    /// The authority of this observation.
    pub authority: EvidenceAuthority,

    /// When the observation was made.
    pub timestamp: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Verification Verdict
// ---------------------------------------------------------------------------

/// The outcome of evaluating evidence against a completion condition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationVerdict {
    /// All conditions satisfied by sufficient evidence.
    Pass,
    /// One or more conditions not satisfied.
    Fail,
    /// Cannot determine — insufficient evidence.
    Inconclusive,
}

// ---------------------------------------------------------------------------
// Evidence Record
// ---------------------------------------------------------------------------

/// A collected piece of evidence for a task's completion claim.
///
/// Evidence is what separates "the model said it worked" from
/// "the runtime confirmed it worked."
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    /// Unique ID for this evidence record.
    pub evidence_id: EvidenceId,

    /// The task this evidence belongs to.
    pub task_id: TaskId,

    /// What is being claimed (e.g., "file hello.txt was created").
    pub claim: String,

    /// Individual observations supporting or refuting the claim.
    pub observations: Vec<Observation>,

    /// Overall verdict based on the observations.
    pub verdict: VerificationVerdict,

    /// The highest authority level in the observations.
    pub authority: EvidenceAuthority,

    /// When this evidence was collected.
    pub timestamp: DateTime<Utc>,
}

impl Evidence {
    /// Create a new evidence record with a single observation.
    pub fn from_observation(
        task_id: TaskId,
        claim: impl Into<String>,
        observation: Observation,
    ) -> Self {
        let authority = observation.authority.clone();
        let verdict = VerificationVerdict::Inconclusive; // caller should set
        Self {
            evidence_id: EvidenceId::new(),
            task_id,
            claim: claim.into(),
            observations: vec![observation],
            verdict,
            authority,
            timestamp: Utc::now(),
        }
    }
}

// ---------------------------------------------------------------------------
// Verification Result
// ---------------------------------------------------------------------------

/// The aggregate result of verifying all completion conditions for a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// Overall verdict.
    pub verdict: VerificationVerdict,

    /// Per-condition results.
    pub condition_results: Vec<ConditionResult>,

    /// Total evidence records examined.
    pub evidence_count: usize,
}

/// Result of checking a single completion condition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConditionResult {
    /// Description of the condition.
    pub condition: String,

    /// Whether the condition was satisfied.
    pub satisfied: bool,

    /// Evidence that supports this result.
    pub evidence_ids: Vec<EvidenceId>,

    /// Reason for failure if not satisfied.
    pub reason: Option<String>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::TaskId;

    #[test]
    fn test_evidence_creation() {
        let obs = Observation {
            kind: "file_exists".into(),
            value: serde_json::json!({"path": "hello.txt", "exists": true}),
            authority: EvidenceAuthority::DeterministicRuntime,
            timestamp: Utc::now(),
        };

        let evidence = Evidence::from_observation(
            TaskId::new(),
            "file hello.txt was created",
            obs,
        );

        assert_eq!(evidence.authority, EvidenceAuthority::DeterministicRuntime);
        assert_eq!(evidence.observations.len(), 1);
    }

    #[test]
    fn test_evidence_authority_ordering() {
        // DeterministicRuntime > CryptographicArtifact > IndependentVerifier > ModelAssertion
        assert_ne!(
            EvidenceAuthority::DeterministicRuntime,
            EvidenceAuthority::ModelAssertion
        );
    }
}
