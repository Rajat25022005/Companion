use chrono::Utc;
use companion_domain::{
    ProcedureStep, Skill, SkillEvalCase, SkillLifecycleState, SkillProvenance,
    SkillTrigger, TaskContract, TaskState,
};
use companion_events::{TaskEvent, TaskEventType};
use tracing::info;

/// Synthesizer that extracts procedural skill candidates from task execution traces.
pub struct SkillSynthesizer;

impl SkillSynthesizer {
    pub fn new() -> Self {
        Self
    }

    /// Mine execution events to synthesize a new candidate skill.
    pub fn synthesize_from_task(
        &self,
        skill_name: &str,
        contract: &TaskContract,
        final_state: &TaskState,
        events: &[TaskEvent],
    ) -> Option<Skill> {
        if !matches!(final_state, TaskState::Completed) {
            return None;
        }

        let mut steps = Vec::new();
        let mut required_caps = Vec::new();
        let mut step_num = 1;

        for event in events {
            if event.event_type == TaskEventType::ToolCallCompleted {
                if let Some(tool_name) = event.payload.get("name").and_then(|v| v.as_str()) {
                    let desc = format!("Invoke tool `{tool_name}` to execute necessary task action");
                    let mut step = ProcedureStep::new(
                        format!("step_{step_num}"),
                        format!("Execute {tool_name}"),
                        desc,
                    )
                    .with_capability(tool_name);

                    if let Some(args) = event.payload.get("args") {
                        step.input_template = Some(args.clone());
                    }

                    if !required_caps.contains(&tool_name.to_string()) {
                        required_caps.push(tool_name.to_string());
                    }

                    steps.push(step);
                    step_num += 1;
                }
            }
        }

        if steps.is_empty() {
            return None;
        }

        let description = format!(
            "Automated procedure for '{}', requiring capabilities: {:?}",
            contract.objective, required_caps
        );

        let triggers = vec![SkillTrigger {
            intent: Some(contract.objective.clone()),
            keywords: contract
                .objective
                .split_whitespace()
                .filter(|w| w.len() > 3)
                .map(|w| w.to_lowercase())
                .collect(),
            required_capabilities: required_caps.clone(),
            mode: Some(format!("{:?}", contract.mode_profile.primary)),
        }];

        let eval_suite = vec![SkillEvalCase {
            case_id: "eval_baseline".into(),
            name: format!("Verify {} execution", skill_name),
            input: contract.user_input.clone(),
            expected_capabilities: required_caps.clone(),
            expected_output_contains: vec![],
            max_turns: (steps.len() as u32) + 2,
            max_cost: 0.05,
        }];

        let provenance = SkillProvenance {
            created_by: "evolution_plane:trace_miner".into(),
            source_task_ids: vec![contract.task_id],
            parent_version: None,
            created_at: Utc::now(),
        };

        let skill = Skill::new(skill_name, 1, description)
            .with_steps(steps)
            .with_capabilities(required_caps)
            .with_triggers(triggers)
            .with_eval_suite(eval_suite)
            .with_state(SkillLifecycleState::Candidate)
            .with_provenance(provenance);

        info!(
            skill = %skill.name,
            steps_count = skill.procedure_graph.len(),
            "synthesized new procedural skill candidate from task trace"
        );

        Some(skill)
    }
}

impl Default for SkillSynthesizer {
    fn default() -> Self {
        Self::new()
    }
}
