use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::{SkillId, TaskId};

// ---------------------------------------------------------------------------
// Skill Lifecycle State
// ---------------------------------------------------------------------------

/// Immutable lifecycle state of a versioned skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SkillLifecycleState {
    /// Extracted or mined as a candidate pattern from execution traces.
    #[default]
    Discovered,
    /// Formalized into a structured Skill IR candidate awaiting validation.
    Candidate,
    /// Undergoing schema, capability, and prerequisite validation.
    Validating,
    /// Executing automated evaluation and regression benchmark suite.
    Evaluating,
    /// Successfully passed evaluation; ready for canary deployment.
    Staged,
    /// Live in canary mode serving partial production traffic.
    Canary,
    /// Approved and promoted to active production.
    Promoted,
    /// Primary active version serving tasks.
    Active,
    /// Failed evaluation or safety checks.
    Rejected,
    /// Rolled back due to runtime metric degradation.
    RolledBack,
    /// Replaced by a newer active version.
    Deprecated,
}

impl SkillLifecycleState {
    /// True if the skill is eligible for active execution in runtime.
    pub fn is_executable(&self) -> bool {
        matches!(self, Self::Active | Self::Canary | Self::Promoted)
    }

    /// Check if state transition is permissible.
    pub fn can_transition_to(&self, next: Self) -> bool {
        match (self, next) {
            (Self::Discovered, Self::Candidate) => true,
            (Self::Candidate, Self::Validating) => true,
            (Self::Candidate, Self::Rejected) => true,
            (Self::Validating, Self::Evaluating) => true,
            (Self::Validating, Self::Rejected) => true,
            (Self::Evaluating, Self::Staged) => true,
            (Self::Evaluating, Self::Rejected) => true,
            (Self::Staged, Self::Canary) => true,
            (Self::Staged, Self::Promoted) => true,
            (Self::Canary, Self::Promoted) => true,
            (Self::Canary, Self::RolledBack) => true,
            (Self::Promoted, Self::Active) => true,
            (Self::Active, Self::Deprecated) => true,
            (Self::Active, Self::RolledBack) => true,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Skill Procedure Graph & Steps
// ---------------------------------------------------------------------------

/// A discrete procedural step in a skill's execution graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcedureStep {
    pub step_id: String,
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_capability: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_template: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_output: Option<String>,
    pub allow_retry: bool,
}

impl ProcedureStep {
    pub fn new(step_id: impl Into<String>, name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            step_id: step_id.into(),
            name: name.into(),
            description: description.into(),
            required_capability: None,
            input_template: None,
            expected_output: None,
            allow_retry: true,
        }
    }

    pub fn with_capability(mut self, cap: impl Into<String>) -> Self {
        self.required_capability = Some(cap.into());
        self
    }
}

/// A decision point in the procedure graph for branching logic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionPoint {
    pub condition: String,
    pub on_true_step: String,
    pub on_false_step: String,
}

/// Trigger definition for automated skill selection.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillTrigger {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

/// Concrete example showing input, execution path, and expected outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillExample {
    pub title: String,
    pub user_input: String,
    pub actions_taken: Vec<String>,
    pub outcome: String,
}

// ---------------------------------------------------------------------------
// Skill Evaluation & Benchmarks
// ---------------------------------------------------------------------------

/// An evaluation test case for verifying skill quality, correctness, and safety.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillEvalCase {
    pub case_id: String,
    pub name: String,
    pub input: String,
    #[serde(default)]
    pub expected_capabilities: Vec<String>,
    #[serde(default)]
    pub expected_output_contains: Vec<String>,
    pub max_turns: u32,
    pub max_cost: f32,
}

/// Result of evaluating a skill against a test case.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillEvalResult {
    pub case_id: String,
    pub passed: bool,
    pub score: f32,
    pub turns_used: u32,
    pub cost_incurred: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Skill Metrics & Provenance
// ---------------------------------------------------------------------------

/// Runtime operational and quality metrics for a versioned skill.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillMetrics {
    pub executions_count: u64,
    pub success_count: u64,
    pub verification_pass_count: u64,
    pub repair_count: u64,
    pub total_tokens: u64,
    pub total_cost: f32,
    pub human_corrections: u32,
    pub safety_score: f32,
}

impl SkillMetrics {
    pub fn success_rate(&self) -> f32 {
        if self.executions_count == 0 {
            0.0
        } else {
            self.success_count as f32 / self.executions_count as f32
        }
    }

    pub fn verification_pass_rate(&self) -> f32 {
        if self.executions_count == 0 {
            0.0
        } else {
            self.verification_pass_count as f32 / self.executions_count as f32
        }
    }

    pub fn repair_rate(&self) -> f32 {
        if self.executions_count == 0 {
            0.0
        } else {
            self.repair_count as f32 / self.executions_count as f32
        }
    }

    pub fn record_execution(
        &mut self,
        success: bool,
        verified: bool,
        repaired: bool,
        tokens: u64,
        cost: f32,
    ) {
        self.executions_count += 1;
        if success {
            self.success_count += 1;
        }
        if verified {
            self.verification_pass_count += 1;
        }
        if repaired {
            self.repair_count += 1;
        }
        self.total_tokens += tokens;
        self.total_cost += cost;
    }
}

/// Provenance metadata tracking the origin and evolution of a skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillProvenance {
    pub created_by: String,
    #[serde(default)]
    pub source_task_ids: Vec<TaskId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_version: Option<u32>,
    pub created_at: DateTime<Utc>,
}

impl Default for SkillProvenance {
    fn default() -> Self {
        Self {
            created_by: "system".into(),
            source_task_ids: Vec::new(),
            parent_version: None,
            created_at: Utc::now(),
        }
    }
}

// ---------------------------------------------------------------------------
// Skill IR (Complete Model)
// ---------------------------------------------------------------------------

/// Versioned procedural skill intermediate representation (Skill IR).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub id: SkillId,
    pub name: String,
    pub version: u32,
    pub description: String,
    #[serde(default)]
    pub triggers: Vec<SkillTrigger>,
    #[serde(default)]
    pub prerequisites: Vec<String>,
    #[serde(default)]
    pub procedure_graph: Vec<ProcedureStep>,
    #[serde(default)]
    pub decision_points: Vec<DecisionPoint>,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
    #[serde(default)]
    pub policies: Vec<String>,
    #[serde(default)]
    pub examples: Vec<SkillExample>,
    #[serde(default)]
    pub evaluation_suite: Vec<SkillEvalCase>,
    pub metrics: SkillMetrics,
    pub provenance: SkillProvenance,
    pub lifecycle_state: SkillLifecycleState,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Alias for Skill matching specification nomenclature.
pub type SkillIR = Skill;

impl Skill {
    pub fn new(name: impl Into<String>, version: u32, description: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: SkillId::new(),
            name: name.into(),
            version,
            description: description.into(),
            triggers: Vec::new(),
            prerequisites: Vec::new(),
            procedure_graph: Vec::new(),
            decision_points: Vec::new(),
            required_capabilities: Vec::new(),
            policies: Vec::new(),
            examples: Vec::new(),
            evaluation_suite: Vec::new(),
            metrics: SkillMetrics {
                safety_score: 1.0,
                ..Default::default()
            },
            provenance: SkillProvenance::default(),
            lifecycle_state: SkillLifecycleState::Discovered,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn with_steps(mut self, steps: Vec<ProcedureStep>) -> Self {
        self.procedure_graph = steps;
        self
    }

    pub fn with_capabilities(mut self, capabilities: Vec<String>) -> Self {
        self.required_capabilities = capabilities;
        self
    }

    pub fn with_triggers(mut self, triggers: Vec<SkillTrigger>) -> Self {
        self.triggers = triggers;
        self
    }

    pub fn with_eval_suite(mut self, eval_cases: Vec<SkillEvalCase>) -> Self {
        self.evaluation_suite = eval_cases;
        self
    }

    pub fn with_state(mut self, state: SkillLifecycleState) -> Self {
        self.lifecycle_state = state;
        self
    }

    pub fn with_provenance(mut self, provenance: SkillProvenance) -> Self {
        self.provenance = provenance;
        self
    }

    /// Check if this skill matches a task's intent or required capabilities.
    pub fn matches_task(&self, intent: &str, capabilities: &[String]) -> bool {
        let intent_lower = intent.to_lowercase();

        // 1. Direct name match
        if intent_lower.contains(&self.name.to_lowercase()) {
            return true;
        }

        // 2. Trigger keywords match
        for trigger in &self.triggers {
            for kw in &trigger.keywords {
                if intent_lower.contains(&kw.to_lowercase()) {
                    return true;
                }
            }
            if let Some(ref t_intent) = trigger.intent {
                if intent_lower.contains(&t_intent.to_lowercase()) {
                    return true;
                }
            }
        }

        // 3. Capabilities overlap match
        if !self.required_capabilities.is_empty() && !capabilities.is_empty() {
            let matches_all_caps = self
                .required_capabilities
                .iter()
                .any(|req| capabilities.contains(req));
            if matches_all_caps {
                return true;
            }
        }

        false
    }
}

// ---------------------------------------------------------------------------
// Skill Promotion Criteria
// ---------------------------------------------------------------------------

/// Thresholds required for a candidate skill to be promoted to active production.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillPromotionCriteria {
    pub min_eval_pass_rate: f32,
    pub min_safety_score: f32,
    pub max_repair_rate: f32,
    pub max_cost_increase_pct: f32,
}

impl Default for SkillPromotionCriteria {
    fn default() -> Self {
        Self {
            min_eval_pass_rate: 0.90,
            min_safety_score: 0.95,
            max_repair_rate: 0.20,
            max_cost_increase_pct: 20.0,
        }
    }
}
