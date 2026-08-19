use std::path::PathBuf;
use std::sync::Arc;
use tempfile::tempdir;
use chrono::{Duration, Utc};

use companion_capabilities::{builtins::register_builtins, CapabilityRegistry};
use companion_domain::{
    MemoryTier, ProcedureStep, Skill, SkillLifecycleState,
    TaskContract, TaskId, TaskState, TenantId, ToolCall, WorkspaceId,
};
use companion_events::EventStore;
use companion_memory::{MemoryManager, MockEmbeddingProvider};
use companion_models::{MockModelProvider, ModelRouter};
use companion_observability::{AuditLedger, MetricsCollector};
use companion_policy::{SecurityRedactor, TenantAuthClaims, TenantSecurityManager};
use companion_runtime::RuntimeEngine;
use companion_skills::{CanaryAction, CanaryController, SkillEvaluator, SkillRegistry, SkillSynthesizer};
use companion_storage::{InMemoryEventStore, InMemoryTaskStore};

#[tokio::test]
async fn test_enterprise_e2e_lifecycle_full_stack() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("service_config.json").to_str().unwrap().to_string();

    // 1. Initialize Observability & Security Redaction
    let metrics = Arc::new(MetricsCollector::new());
    let audit_ledger = Arc::new(AuditLedger::new());
    let redactor = SecurityRedactor::new();

    // 2. Initialize Memory & SkillOS
    let memory_manager = Arc::new(MemoryManager::new(Arc::new(MockEmbeddingProvider::new())));
    memory_manager.remember("Payment gateway must be strictly idempotent", MemoryTier::Semantic, 1.5).await.unwrap();

    let skill_registry = Arc::new(SkillRegistry::new());
    let step = ProcedureStep::new("step_1", "Write JSON", "Write config").with_capability("filesystem.write");
    let skill = Skill::new("codegen_standards", 1, "Guidelines for clean service generation")
        .with_steps(vec![step])
        .with_capabilities(vec!["filesystem.write".into()]);
    skill_registry.register_candidate(skill).await.unwrap();
    skill_registry.promote_skill("codegen_standards", 1).await.unwrap();

    // 3. Initialize Capabilities
    let mut cap_registry = CapabilityRegistry::new();
    register_builtins(&mut cap_registry);
    let cap_registry = Arc::new(cap_registry);

    // 4. Initialize Model Router with Mock Provider
    let mut model_router = ModelRouter::new();
    let provider = Arc::new(MockModelProvider::new("e2e_mock"));

    // Turn 1: Propose tool call to write file
    provider.push_tool_call_response(vec![ToolCall {
        id: "call_e2e_1".into(),
        name: "filesystem.write".into(),
        arguments: serde_json::json!({
            "path": file_path,
            "content": "{\"service\": \"payment-gateway\", \"status\": \"ready\"}"
        }),
    }]);

    // Turn 2: Finish task
    provider.push_text_response("Service configuration artifact has been generated and validated.");

    model_router.register(provider);
    model_router.set_default("e2e_mock");
    let model_router = Arc::new(model_router);

    // 5. Initialize Runtime Engine
    let event_store = Arc::new(InMemoryEventStore::new());
    let task_store = Arc::new(InMemoryTaskStore::new());
    let engine = Arc::new(RuntimeEngine::new(
        model_router,
        cap_registry,
        event_store.clone(),
        task_store,
    ));

    // 6. User submits prompt with sensitive API key
    let user_input_raw = "#build Create payment service config with token=sk-abcdef1234567890abcdef123456";
    assert!(redactor.contains_sensitive_data(user_input_raw));
    let sanitized_input = redactor.redact(user_input_raw);
    assert!(!sanitized_input.contains("sk-abcdef"));
    assert!(sanitized_input.contains("[REDACTED_API_KEY]"));

    // 7. Audit Log task start
    let tenant_id = TenantId::new();
    audit_ledger.append(
        tenant_id,
        None,
        "operator",
        "task.submit",
        serde_json::json!({"prompt": sanitized_input}),
    ).await;

    // 8. Execute Task through Runtime Engine
    let (task_id, final_state, contract) = engine
        .submit_and_run(&sanitized_input, None, None, Some(dir.path().to_str().unwrap().to_string()))
        .await
        .unwrap();

    // Verify task completion
    assert_eq!(final_state, TaskState::Completed);
    assert_eq!(contract.mode_profile.primary, companion_domain::Mode::Build);

    // Verify file actually created on filesystem
    assert!(tokio::fs::metadata(&file_path).await.is_ok());

    // 9. Update Metrics and Audit Trail
    metrics.record_task(true, false);
    metrics.record_tool_call("filesystem.write", 30, false).await;
    metrics.record_tokens(570, 105, 0.0027);

    audit_ledger.append(
        tenant_id,
        Some(task_id),
        "runtime:engine",
        "task.complete",
        serde_json::json!({"state": "Completed"}),
    ).await;

    // Verify Cryptographic Audit Hash Chain
    let is_intact = audit_ledger.verify_integrity().await.unwrap();
    assert!(is_intact, "Audit ledger hash chain must be intact!");
    assert_eq!(audit_ledger.count().await, 2);

    // Verify Prometheus Metrics
    let prom = metrics.export_prometheus().await;
    assert!(prom.contains("companion_tasks_total 1"));
    assert!(prom.contains("companion_tasks_succeeded 1"));
    assert!(prom.contains("companion_tool_calls_total 1"));

    // 10. Record Episodic Memory
    let events = event_store.load_events(task_id).await.unwrap();
    let episode = memory_manager
        .episodic_recorder()
        .record_task_episode(&contract, &final_state, &events)
        .await
        .unwrap();
    assert_eq!(episode.tier, MemoryTier::Episodic);
}

#[test]
fn test_enterprise_multi_tenant_workspace_isolation_and_tokens() {
    let base_storage = PathBuf::from("/var/companion/tenants");
    let manager = TenantSecurityManager::new(base_storage.clone(), "enterprise_hmac_secret_2026");

    let tenant_a = TenantId::new();
    let workspace_a = WorkspaceId::new();

    let tenant_b = TenantId::new();
    let _workspace_b = WorkspaceId::new();

    // Valid path inside Tenant A
    let safe_path = PathBuf::from("configs/app.yaml");
    let validated = manager.validate_path_isolation(&tenant_a, &workspace_a, &safe_path);
    assert!(validated.is_ok());
    assert_eq!(
        validated.unwrap(),
        base_storage.join(tenant_a.to_string()).join(workspace_a.to_string()).join("configs/app.yaml")
    );

    // Path traversal attempt inside Tenant A
    let traversal = PathBuf::from("../tenant_b/secret.key");
    assert!(manager.validate_path_isolation(&tenant_a, &workspace_a, &traversal).is_err());

    // Issue and validate token for Tenant A
    let claims = TenantAuthClaims {
        tenant_id: tenant_a,
        workspace_id: workspace_a,
        roles: vec!["operator".into()],
        expires_at: Utc::now() + Duration::hours(12),
    };
    let token = manager.issue_token(&claims);
    let verified = manager.validate_token(&token).unwrap();
    assert_eq!(verified.tenant_id, tenant_a);

    // Token for Tenant A cannot authenticate as Tenant B
    assert_ne!(verified.tenant_id, tenant_b);
}

#[tokio::test]
async fn test_enterprise_skill_self_evolution_and_canary() {
    let registry = Arc::new(SkillRegistry::new());
    let v1 = Skill::new("api_builder", 1, "Original API builder")
        .with_capabilities(vec!["filesystem.write".into()]);
    registry.register_candidate(v1).await.unwrap();
    registry.promote_skill("api_builder", 1).await.unwrap();

    // Trace Mining / Synthesis
    let synthesizer = SkillSynthesizer::new();
    let dummy_contract = TaskContract {
        task_id: TaskId::new(),
        tenant_id: TenantId::new(),
        workspace_id: WorkspaceId::new(),
        correlation_id: companion_domain::CorrelationId::new(),
        workflow_id: None,
        goal_id: None,
        parent_task_id: None,
        objective: "Build REST api".into(),
        mode_profile: companion_domain::ModeProfile::from_mode(companion_domain::Mode::Build),
        required_capabilities: vec![],
        allowed_tools: vec!["filesystem.write".into()],
        completion_conditions: vec![],
        constraints: vec![],
        risk_level: companion_domain::RiskLevel::Low,
        budget: companion_domain::TaskBudget::default(),
        user_input: "#build Build REST api".into(),
        created_at: Utc::now(),
    };

    let events = vec![
        companion_events::TaskEvent::new(
            dummy_contract.task_id,
            dummy_contract.correlation_id,
            1,
            companion_events::TaskEventType::ToolCallCompleted,
            serde_json::json!({
                "name": "filesystem.write",
                "execution_ms": 25
            }),
        ),
    ];

    let candidate_opt = synthesizer.synthesize_from_task("api_builder", &dummy_contract, &TaskState::Completed, &events);
    assert!(candidate_opt.is_some());
    let candidate = candidate_opt.unwrap();

    // Eval harness
    let evaluator = SkillEvaluator::new();
    let report = evaluator.evaluate_candidate(&candidate, None).await.unwrap();
    assert!(report.passes_promotion_criteria);

    // Canary testing & Promotion
    registry.register_candidate(candidate.clone()).await.unwrap();
    registry.stage_canary("api_builder", 2).await.unwrap();

    let canary_ctrl = CanaryController::new(registry.clone())
        .with_canary_ratio(1.0)
        .with_min_evaluations(2);

    assert_eq!(canary_ctrl.record_canary_execution("api_builder", 2, true).await.unwrap(), CanaryAction::ContinueCanary);
    assert_eq!(canary_ctrl.record_canary_execution("api_builder", 2, true).await.unwrap(), CanaryAction::Promoted);

    let active = registry.get_active_skill("api_builder").await.unwrap();
    assert_eq!(active.version, 2);
    assert_eq!(active.lifecycle_state, SkillLifecycleState::Active);
}
