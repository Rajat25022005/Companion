use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use companion_domain::{Evidence, TaskContract, TaskState};
use companion_events::{TaskEvent, TaskEventType};

// ---------------------------------------------------------------------------
// Error Taxonomy — Semantic classification of failure root causes
// ---------------------------------------------------------------------------

/// Categorized root cause of a task failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "category", rename_all = "snake_case")]
pub enum ErrorTaxonomy {
    /// A required file, tool, or capability was not found.
    MissingDependency { message: String },
    /// An upstream API or service throttled the request.
    RateLimited { message: String },
    /// Authorization or filesystem permission was denied.
    PermissionDenied { message: String },
    /// A tool produced malformed or unexpected output.
    InvalidOutput { message: String },
    /// An operation exceeded its deadline.
    TimeoutExceeded { message: String },
    /// The model refused to generate the required content.
    ModelRefusal { message: String },
    /// Verification conditions failed after maximum repair attempts.
    VerificationExhausted { message: String },
    /// Unclassifiable failure.
    Unknown { message: String },
}

impl ErrorTaxonomy {
    /// Whether this error category is potentially recoverable via compensation.
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Self::RateLimited { .. }
                | Self::InvalidOutput { .. }
                | Self::TimeoutExceeded { .. }
                | Self::ModelRefusal { .. }
        )
    }

    /// Short label for the error category.
    pub fn category_label(&self) -> &'static str {
        match self {
            Self::MissingDependency { .. } => "missing_dependency",
            Self::RateLimited { .. } => "rate_limited",
            Self::PermissionDenied { .. } => "permission_denied",
            Self::InvalidOutput { .. } => "invalid_output",
            Self::TimeoutExceeded { .. } => "timeout_exceeded",
            Self::ModelRefusal { .. } => "model_refusal",
            Self::VerificationExhausted { .. } => "verification_exhausted",
            Self::Unknown { .. } => "unknown",
        }
    }
}

// ---------------------------------------------------------------------------
// Compensation Plan
// ---------------------------------------------------------------------------

/// A structured autonomous recovery action.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum CompensationAction {
    /// Retry the failed operation with exponential backoff.
    RetryWithBackoff { tool: String, delay_ms: u64 },
    /// Substitute a failed capability with an alternative.
    SubstituteCapability { original: String, replacement: String },
    /// Inject additional context into the prompt to guide the model.
    InjectContext { additional_prompt: String },
    /// Escalate to human operator — cannot be auto-resolved.
    EscalateToHuman { reason: String },
}

/// A plan for autonomous recovery from failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompensationPlan {
    pub actions: Vec<CompensationAction>,
    pub max_retry_budget: u32,
    pub estimated_success_probability: f64,
}

// ---------------------------------------------------------------------------
// Failure Diagnosis
// ---------------------------------------------------------------------------

/// Structured output from the FailureAnalyzer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureDiagnosis {
    pub root_cause: ErrorTaxonomy,
    pub contributing_factors: Vec<String>,
    pub compensation_plan: Option<CompensationPlan>,
    pub analyzed_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Failure Analyzer
// ---------------------------------------------------------------------------

/// Inspects a failed task's event trail and evidence chain to produce
/// a structured diagnosis with root cause classification.
pub struct FailureAnalyzer;

impl FailureAnalyzer {
    pub fn new() -> Self {
        Self
    }

    /// Analyze a failed task to produce a structured diagnosis.
    pub fn analyze(
        &self,
        _contract: &TaskContract,
        events: &[TaskEvent],
        _evidence: &[Evidence],
        final_state: &TaskState,
    ) -> FailureDiagnosis {
        let failure_reason = match final_state {
            TaskState::Failed { reason } => reason.clone(),
            _ => "Unknown failure state".to_string(),
        };

        let reason_lower = failure_reason.to_lowercase();

        // Classify root cause from failure reason and event patterns
        let root_cause = if reason_lower.contains("rate limit") || reason_lower.contains("429")
            || reason_lower.contains("throttle")
        {
            ErrorTaxonomy::RateLimited {
                message: failure_reason.clone(),
            }
        } else if reason_lower.contains("permission")
            || reason_lower.contains("denied")
            || reason_lower.contains("unauthorized")
            || reason_lower.contains("forbidden")
        {
            ErrorTaxonomy::PermissionDenied {
                message: failure_reason.clone(),
            }
        } else if reason_lower.contains("not found")
            || reason_lower.contains("missing")
            || reason_lower.contains("no such file")
        {
            ErrorTaxonomy::MissingDependency {
                message: failure_reason.clone(),
            }
        } else if reason_lower.contains("timeout")
            || reason_lower.contains("timed out")
            || reason_lower.contains("deadline")
        {
            ErrorTaxonomy::TimeoutExceeded {
                message: failure_reason.clone(),
            }
        } else if reason_lower.contains("refus") || reason_lower.contains("cannot assist") {
            ErrorTaxonomy::ModelRefusal {
                message: failure_reason.clone(),
            }
        } else if reason_lower.contains("verification failed")
            || reason_lower.contains("repair attempt")
        {
            ErrorTaxonomy::VerificationExhausted {
                message: failure_reason.clone(),
            }
        } else if reason_lower.contains("invalid") || reason_lower.contains("malformed") {
            ErrorTaxonomy::InvalidOutput {
                message: failure_reason.clone(),
            }
        } else {
            ErrorTaxonomy::Unknown {
                message: failure_reason.clone(),
            }
        };

        // Gather contributing factors from events
        let mut contributing_factors = Vec::new();

        let failed_tool_count = events
            .iter()
            .filter(|e| e.event_type == TaskEventType::ToolCallFailed)
            .count();
        if failed_tool_count > 0 {
            contributing_factors.push(format!("{failed_tool_count} tool call(s) failed during execution"));
        }

        let rejection_count = events
            .iter()
            .filter(|e| e.event_type == TaskEventType::TurnRejected)
            .count();
        if rejection_count > 0 {
            contributing_factors.push(format!("{rejection_count} turn(s) rejected by policy monitor"));
        }

        let repair_count = events
            .iter()
            .filter(|e| e.event_type == TaskEventType::VerificationFailed)
            .count();
        if repair_count > 0 {
            contributing_factors.push(format!("{repair_count} verification failure(s) triggered repairs"));
        }

        // Build compensation plan if recoverable
        let compensation_plan = if root_cause.is_recoverable() {
            let actions = match &root_cause {
                ErrorTaxonomy::RateLimited { .. } => {
                    vec![CompensationAction::RetryWithBackoff {
                        tool: "model_call".into(),
                        delay_ms: 2000,
                    }]
                }
                ErrorTaxonomy::TimeoutExceeded { .. } => {
                    vec![CompensationAction::RetryWithBackoff {
                        tool: "operation".into(),
                        delay_ms: 5000,
                    }]
                }
                ErrorTaxonomy::ModelRefusal { .. } => {
                    vec![CompensationAction::InjectContext {
                        additional_prompt:
                            "Please rephrase the approach. The previous attempt was not accepted. \
                             Try an alternative method to accomplish the objective."
                                .into(),
                    }]
                }
                ErrorTaxonomy::InvalidOutput { .. } => {
                    vec![CompensationAction::InjectContext {
                        additional_prompt:
                            "The previous tool output was malformed. Please retry the operation \
                             and ensure the output conforms to the expected format."
                                .into(),
                    }]
                }
                _ => vec![],
            };

            Some(CompensationPlan {
                actions,
                max_retry_budget: 2,
                estimated_success_probability: 0.7,
            })
        } else {
            None
        };

        info!(
            root_cause = root_cause.category_label(),
            recoverable = root_cause.is_recoverable(),
            factors = contributing_factors.len(),
            "failure analysis complete"
        );

        FailureDiagnosis {
            root_cause,
            contributing_factors,
            compensation_plan,
            analyzed_at: Utc::now(),
        }
    }
}

// ---------------------------------------------------------------------------
// Self-Healing Loop
// ---------------------------------------------------------------------------

/// Orchestrates autonomous recovery from failed task states.
pub struct SelfHealingLoop {
    analyzer: FailureAnalyzer,
    max_healing_attempts: u32,
}

impl SelfHealingLoop {
    pub fn new() -> Self {
        Self {
            analyzer: FailureAnalyzer::new(),
            max_healing_attempts: 2,
        }
    }

    pub fn with_max_attempts(mut self, max: u32) -> Self {
        self.max_healing_attempts = max;
        self
    }

    /// Attempt to diagnose and generate a recovery plan for a failed task.
    ///
    /// Returns `Some(diagnosis)` if healing should be attempted, `None` if the
    /// failure is non-recoverable or healing budget is exhausted.
    pub fn attempt_healing(
        &self,
        contract: &TaskContract,
        events: &[TaskEvent],
        evidence: &[Evidence],
        final_state: &TaskState,
        previous_attempts: u32,
    ) -> Option<FailureDiagnosis> {
        if previous_attempts >= self.max_healing_attempts {
            warn!(
                task_id = %contract.task_id,
                attempts = previous_attempts,
                max = self.max_healing_attempts,
                "self-healing budget exhausted"
            );
            return None;
        }

        let diagnosis = self.analyzer.analyze(contract, events, evidence, final_state);

        if diagnosis.root_cause.is_recoverable() && diagnosis.compensation_plan.is_some() {
            info!(
                task_id = %contract.task_id,
                root_cause = diagnosis.root_cause.category_label(),
                attempt = previous_attempts + 1,
                "self-healing: recoverable failure detected, compensation plan generated"
            );
            Some(diagnosis)
        } else {
            info!(
                task_id = %contract.task_id,
                root_cause = diagnosis.root_cause.category_label(),
                "self-healing: non-recoverable failure, no compensation available"
            );
            None
        }
    }
}
