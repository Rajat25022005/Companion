use std::sync::Arc;
use companion_domain::{
    ProcedureStep, Skill, SkillEvalCase, SkillLifecycleState, SkillPromotionCriteria,
    TaskContract, TaskId, TaskState,
};
use companion_events::{TaskEvent, TaskEventType};
use companion_skills::{
    CanaryAction, CanaryController, SkillEvaluator, SkillMarkdownParser, SkillRegistry,
    SkillSynthesizer,
};

#[tokio::test]
async fn test_skill_registry_immutability_and_versioning() {
    let registry = SkillRegistry::new();

    // 1. Register candidate v1
    let s1 = Skill::new("deploy-app", 1, "Deploy application to cloud")
        .with_capabilities(vec!["process.execute".into()]);
    let reg_v1 = registry.register_candidate(s1).await.unwrap();
    assert_eq!(reg_v1.version, 1);
    assert_eq!(reg_v1.lifecycle_state, SkillLifecycleState::Candidate);

    // 2. Promote v1 to active
    let act_v1 = registry.promote_skill("deploy-app", 1).await.unwrap();
    assert_eq!(act_v1.lifecycle_state, SkillLifecycleState::Active);

    // 3. Register candidate v2
    let s2 = Skill::new("deploy-app", 1, "Optimized deploy application to cloud")
        .with_capabilities(vec!["process.execute".into(), "network.http".into()]);
    let reg_v2 = registry.register_candidate(s2).await.unwrap();

    // Invariant: v2 is version 2, parent is 1, state is Candidate
    assert_eq!(reg_v2.version, 2);
    assert_eq!(reg_v2.provenance.parent_version, Some(1));
    assert_eq!(reg_v2.lifecycle_state, SkillLifecycleState::Candidate);

    // Invariant: v1 remains the active skill while v2 is a candidate
    let current_active = registry.get_active_skill("deploy-app").await.unwrap();
    assert_eq!(current_active.version, 1);
    assert_eq!(current_active.lifecycle_state, SkillLifecycleState::Active);
}

#[tokio::test]
async fn test_skill_promotion_and_deprecation() {
    let registry = SkillRegistry::new();

    let s1 = Skill::new("test-runner", 1, "Run test suite");
    registry.register_candidate(s1).await.unwrap();
    registry.promote_skill("test-runner", 1).await.unwrap();

    let s2 = Skill::new("test-runner", 1, "Parallel test runner");
    registry.register_candidate(s2).await.unwrap();

    // Promote v2
    let promoted_v2 = registry.promote_skill("test-runner", 2).await.unwrap();
    assert_eq!(promoted_v2.version, 2);
    assert_eq!(promoted_v2.lifecycle_state, SkillLifecycleState::Active);

    // Verify v1 is now Deprecated
    let v1 = registry.get_skill_version("test-runner", 1).await.unwrap();
    assert_eq!(v1.lifecycle_state, SkillLifecycleState::Deprecated);

    // Verify get_active_skill returns v2
    let active = registry.get_active_skill("test-runner").await.unwrap();
    assert_eq!(active.version, 2);
}

#[tokio::test]
async fn test_skill_rollback_restores_previous_stable_version() {
    let registry = SkillRegistry::new();

    // Set up v1 Active -> v2 Active (v1 Deprecated)
    let s1 = Skill::new("db-migrator", 1, "Run migrations");
    registry.register_candidate(s1).await.unwrap();
    registry.promote_skill("db-migrator", 1).await.unwrap();

    let s2 = Skill::new("db-migrator", 1, "Run async migrations");
    registry.register_candidate(s2).await.unwrap();
    registry.promote_skill("db-migrator", 2).await.unwrap();

    // Trigger Rollback on v2
    let restored = registry
        .rollback_skill("db-migrator", "Runtime crashes detected in v2")
        .await
        .unwrap();

    // Invariant: v1 is restored to Active
    assert_eq!(restored.version, 1);
    assert_eq!(restored.lifecycle_state, SkillLifecycleState::Active);

    // Invariant: v2 is marked RolledBack
    let v2 = registry.get_skill_version("db-migrator", 2).await.unwrap();
    assert_eq!(v2.lifecycle_state, SkillLifecycleState::RolledBack);
}

#[tokio::test]
async fn test_skill_evaluator_regression_and_safety() {
    let evaluator = SkillEvaluator::new().with_criteria(SkillPromotionCriteria {
        min_eval_pass_rate: 0.9,
        min_safety_score: 0.9,
        max_repair_rate: 0.2,
        max_cost_increase_pct: 20.0,
    });

    // 1. Valid candidate
    let step1 = ProcedureStep::new("step_1", "Read file", "Read config").with_capability("filesystem.read");
    let eval_case = SkillEvalCase {
        case_id: "case_1".into(),
        name: "Test Read".into(),
        input: "Read file".into(),
        expected_capabilities: vec!["filesystem.read".into()],
        expected_output_contains: vec![],
        max_turns: 3,
        max_cost: 0.05,
    };

    let valid_skill = Skill::new("file-reader", 1, "Read configuration file")
        .with_steps(vec![step1])
        .with_capabilities(vec!["filesystem.read".into()])
        .with_eval_suite(vec![eval_case]);

    let report = evaluator.evaluate_candidate(&valid_skill, None).await.unwrap();
    assert!(report.passes_promotion_criteria);
    assert_eq!(report.recommendation, SkillLifecycleState::Staged);

    // 2. Unsafe candidate requesting unauthorized capability
    let unsafe_step = ProcedureStep::new("step_x", "Root rm", "Delete root").with_capability("kernel.raw_exec");
    let unsafe_skill = Skill::new("file-reader", 2, "Unsafe variant")
        .with_steps(vec![unsafe_step])
        .with_capabilities(vec!["kernel.raw_exec".into()]);

    let unsafe_report = evaluator.evaluate_candidate(&unsafe_skill, None).await.unwrap();
    assert!(!unsafe_report.passes_promotion_criteria);
    assert_eq!(unsafe_report.recommendation, SkillLifecycleState::Rejected);
}

#[tokio::test]
async fn test_skill_synthesizer_trace_mining() {
    let synthesizer = SkillSynthesizer::new();
    let contract = TaskContract {
        task_id: TaskId::new(),
        tenant_id: companion_domain::TenantId::new(),
        workspace_id: companion_domain::WorkspaceId::new(),
        correlation_id: companion_domain::CorrelationId::new(),
        workflow_id: None,
        goal_id: None,
        parent_task_id: None,
        objective: "Compile Rust crate and run tests".into(),
        mode_profile: companion_domain::ModeProfile::from_mode(companion_domain::Mode::Build),
        required_capabilities: vec![],
        allowed_tools: vec!["filesystem.write".into(), "process.execute".into()],
        completion_conditions: vec![],
        constraints: vec![],
        risk_level: companion_domain::RiskLevel::Low,
        budget: companion_domain::TaskBudget::default(),
        user_input: "#build Compile Rust crate".into(),
        created_at: chrono::Utc::now(),
    };

    let events = vec![
        TaskEvent::new(
            contract.task_id,
            contract.correlation_id,
            1,
            TaskEventType::ToolCallCompleted,
            serde_json::json!({
                "name": "filesystem.write",
                "args": {"path": "src/main.rs"}
            }),
        ),
        TaskEvent::new(
            contract.task_id,
            contract.correlation_id,
            2,
            TaskEventType::ToolCallCompleted,
            serde_json::json!({
                "name": "process.execute",
                "args": {"command": "cargo test"}
            }),
        ),
    ];

    let synthesized = synthesizer
        .synthesize_from_task("cargo-build-test", &contract, &TaskState::Completed, &events)
        .expect("Should synthesize skill candidate");

    assert_eq!(synthesized.name, "cargo-build-test");
    assert_eq!(synthesized.procedure_graph.len(), 2);
    assert_eq!(synthesized.required_capabilities, vec!["filesystem.write", "process.execute"]);
    assert_eq!(synthesized.lifecycle_state, SkillLifecycleState::Candidate);
}

#[tokio::test]
async fn test_skill_markdown_parser_roundtrip() {
    let md = r#"---
name: build-rust-app
version: 1
description: Build and test Rust application
state: Active
capabilities:
  - filesystem.write
  - process.execute
---

# build-rust-app

Build and test Rust application

## Procedure
- [filesystem.write] Create Cargo manifest and source
- [process.execute] Run cargo test
"#;

    let skill = SkillMarkdownParser::parse(md).unwrap();
    assert_eq!(skill.name, "build-rust-app");
    assert_eq!(skill.version, 1);
    assert_eq!(skill.lifecycle_state, SkillLifecycleState::Active);
    assert_eq!(skill.procedure_graph.len(), 2);
    assert_eq!(skill.procedure_graph[0].required_capability.as_deref(), Some("filesystem.write"));

    let serialized = SkillMarkdownParser::serialize(&skill);
    assert!(serialized.contains("name: build-rust-app"));
    assert!(serialized.contains("## Procedure"));
    assert!(serialized.contains("[filesystem.write]"));
}

#[tokio::test]
async fn test_canary_controller_traffic_and_auto_rollback() {
    let registry = Arc::new(SkillRegistry::new());

    // v1 is Active
    let s1 = Skill::new("cache-invalidator", 1, "Invalidate cache");
    registry.register_candidate(s1).await.unwrap();
    registry.promote_skill("cache-invalidator", 1).await.unwrap();

    // v2 is Canary
    let s2 = Skill::new("cache-invalidator", 1, "Async cache invalidator");
    registry.register_candidate(s2).await.unwrap();
    registry.stage_canary("cache-invalidator", 2).await.unwrap();

    let canary_ctrl = CanaryController::new(registry.clone())
        .with_canary_ratio(1.0) // Route all to canary for deterministic test
        .with_min_evaluations(3);

    // Route traffic -> gets v2
    let selected = canary_ctrl.select_version("cache-invalidator", 0.5).await.unwrap();
    assert_eq!(selected.version, 2);

    // Record failure 1
    let res1 = canary_ctrl.record_canary_execution("cache-invalidator", 2, false).await.unwrap();
    assert_eq!(res1, CanaryAction::ContinueCanary);

    // Record failure 2 (triggers rollback: 2 failures / 2 runs = 100% > 20%)
    let res2 = canary_ctrl.record_canary_execution("cache-invalidator", 2, false).await.unwrap();
    assert_eq!(res2, CanaryAction::RolledBack);

    // Verify registry restored v1 to Active
    let active = registry.get_active_skill("cache-invalidator").await.unwrap();
    assert_eq!(active.version, 1);
    assert_eq!(active.lifecycle_state, SkillLifecycleState::Active);
}

#[tokio::test]
async fn test_canary_controller_auto_promotion() {
    let registry = Arc::new(SkillRegistry::new());

    let s1 = Skill::new("log-aggregator", 1, "Aggregate logs");
    registry.register_candidate(s1).await.unwrap();
    registry.promote_skill("log-aggregator", 1).await.unwrap();

    let s2 = Skill::new("log-aggregator", 1, "Stream aggregate logs");
    registry.register_candidate(s2).await.unwrap();
    registry.stage_canary("log-aggregator", 2).await.unwrap();

    let canary_ctrl = CanaryController::new(registry.clone())
        .with_canary_ratio(1.0)
        .with_min_evaluations(3);

    // Record 3 consecutive successes
    assert_eq!(canary_ctrl.record_canary_execution("log-aggregator", 2, true).await.unwrap(), CanaryAction::ContinueCanary);
    assert_eq!(canary_ctrl.record_canary_execution("log-aggregator", 2, true).await.unwrap(), CanaryAction::ContinueCanary);
    assert_eq!(canary_ctrl.record_canary_execution("log-aggregator", 2, true).await.unwrap(), CanaryAction::Promoted);

    // Verify registry promoted v2 to Active
    let active = registry.get_active_skill("log-aggregator").await.unwrap();
    assert_eq!(active.version, 2);
    assert_eq!(active.lifecycle_state, SkillLifecycleState::Active);
}
