use companion_domain::{
    RuntimeError, Skill, SkillEvalResult, SkillLifecycleState, SkillPromotionCriteria,
};
use tracing::{debug, info, warn};

/// Evaluation outcome summary for a candidate skill.
#[derive(Debug, Clone)]
pub struct SkillEvaluationReport {
    pub skill_name: String,
    pub candidate_version: u32,
    pub cases_total: usize,
    pub cases_passed: usize,
    pub pass_rate: f32,
    pub safety_score: f32,
    pub passes_promotion_criteria: bool,
    pub eval_results: Vec<SkillEvalResult>,
    pub recommendation: SkillLifecycleState,
}

/// Automated evaluation engine for skill testing, regression benchmarking, and safety verification.
pub struct SkillEvaluator {
    criteria: SkillPromotionCriteria,
    allowed_capabilities: Vec<String>,
}

impl SkillEvaluator {
    pub fn new() -> Self {
        Self {
            criteria: SkillPromotionCriteria::default(),
            allowed_capabilities: vec![
                "filesystem.read".into(),
                "filesystem.write".into(),
                "process.execute".into(),
                "git.commit".into(),
                "network.http".into(),
            ],
        }
    }

    pub fn with_criteria(mut self, criteria: SkillPromotionCriteria) -> Self {
        self.criteria = criteria;
        self
    }

    pub fn with_allowed_capabilities(mut self, caps: Vec<String>) -> Self {
        self.allowed_capabilities = caps;
        self
    }

    /// Evaluate a candidate skill against its evaluation suite and safety constraints.
    pub async fn evaluate_candidate(
        &self,
        candidate: &Skill,
        baseline: Option<&Skill>,
    ) -> Result<SkillEvaluationReport, RuntimeError> {
        debug!(
            skill = %candidate.name,
            version = candidate.version,
            eval_cases = candidate.evaluation_suite.len(),
            "starting skill candidate evaluation"
        );

        let mut eval_results = Vec::new();
        let mut cases_passed = 0;

        // 1. Safety & Capability Whitelist Verification
        let mut safety_score = candidate.metrics.safety_score;
        for cap in &candidate.required_capabilities {
            if !self.allowed_capabilities.contains(cap) {
                warn!(
                    skill = %candidate.name,
                    unauthorized_capability = %cap,
                    "candidate requested unauthorized capability"
                );
                safety_score *= 0.5; // Penalize safety score
            }
        }

        // 2. Execute Evaluation Suite Test Cases
        for case in &candidate.evaluation_suite {
            let mut passed = true;
            let mut error = None;

            // Verify procedure graph can satisfy expected capabilities
            for expected_cap in &case.expected_capabilities {
                let has_cap = candidate
                    .required_capabilities
                    .contains(expected_cap)
                    || candidate
                        .procedure_graph
                        .iter()
                        .any(|step| step.required_capability.as_ref() == Some(expected_cap));

                if !has_cap {
                    passed = false;
                    error = Some(format!("Missing expected capability `{expected_cap}` in procedure graph"));
                    break;
                }
            }

            // Check max turns constraint
            let turns_used = candidate.procedure_graph.len() as u32;
            if turns_used > case.max_turns {
                passed = false;
                error = Some(format!("Turns used ({turns_used}) exceeds max_turns ({})", case.max_turns));
            }

            if passed {
                cases_passed += 1;
            }

            eval_results.push(SkillEvalResult {
                case_id: case.case_id.clone(),
                passed,
                score: if passed { 1.0 } else { 0.0 },
                turns_used,
                cost_incurred: turns_used as f32 * 0.005,
                error,
            });
        }

        let cases_total = candidate.evaluation_suite.len();
        let pass_rate = if cases_total == 0 {
            1.0
        } else {
            cases_passed as f32 / cases_total as f32
        };

        // 3. Regression Comparison against Baseline (if exists)
        let mut regression_ok = true;
        if let Some(base) = baseline {
            if base.metrics.success_rate() > pass_rate + 0.05 {
                warn!(
                    baseline_rate = base.metrics.success_rate(),
                    candidate_rate = pass_rate,
                    "candidate regressed compared to baseline version"
                );
                regression_ok = false;
            }
        }

        let passes_promotion_criteria = pass_rate >= self.criteria.min_eval_pass_rate
            && safety_score >= self.criteria.min_safety_score
            && regression_ok;

        let recommendation = if passes_promotion_criteria {
            SkillLifecycleState::Staged
        } else {
            SkillLifecycleState::Rejected
        };

        info!(
            skill = %candidate.name,
            version = candidate.version,
            pass_rate,
            safety_score,
            recommendation = ?recommendation,
            "skill evaluation complete"
        );

        Ok(SkillEvaluationReport {
            skill_name: candidate.name.clone(),
            candidate_version: candidate.version,
            cases_total,
            cases_passed,
            pass_rate,
            safety_score,
            passes_promotion_criteria,
            eval_results,
            recommendation,
        })
    }
}

impl Default for SkillEvaluator {
    fn default() -> Self {
        Self::new()
    }
}
