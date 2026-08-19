use std::sync::Arc;
use std::time::Duration;

use companion_agents::AgentTeam;
use companion_cap::CapRouter;
use companion_capabilities::{builtins::register_builtins, CapabilityRegistry};
use companion_domain::{
    AgentAddress, AgentRole, CapEnvelope, CapPayload, CorrelationId, ConversationId,
    Goal, Milestone, MessagePattern, StepId, StepRetryPolicy,
    TenantId, WorkflowDef, WorkflowStatus, WorkflowStep,
};
use companion_models::{MockModelProvider, ModelRouter};
use companion_runtime::RuntimeEngine;
use companion_storage::{InMemoryEventStore, InMemoryTaskStore};
use companion_workflow::WorkflowEngine;

#[tokio::test]
async fn test_multi_agent_delegation() {
    let cap_router = Arc::new(CapRouter::new());
    let coordinator_addr = AgentAddress::new(AgentRole::Coordinator);
    let engineer_addr = AgentAddress::new(AgentRole::Engineer);

    let _coord_mb = cap_router.register_agent(coordinator_addr.clone(), 10).await;
    let eng_mb = cap_router.register_agent(engineer_addr.clone(), 10).await;

    // Coordinator sends Request to Engineer
    let correlation_id = CorrelationId::new();
    let conv_id = ConversationId::new();
    let req = CapEnvelope::new(
        coordinator_addr.clone(),
        engineer_addr.clone(),
        correlation_id,
        conv_id,
        MessagePattern::Request,
        CapPayload::Text {
            content: "Implement login endpoint".into(),
        },
    );

    let req_id = req.message_id;

    // Spawn task to simulate Engineer processing and replying
    let router_clone = cap_router.clone();
    tokio::spawn(async move {
        if let Some(msg) = eng_mb.pop().await {
            let resp = msg.create_response(
                engineer_addr,
                CapPayload::TaskResult {
                    success: true,
                    output: serde_json::json!({"status": "implemented"}),
                    evidence_summary: Some("Tested login with 200 OK".into()),
                },
            );
            router_clone.route(resp).await.unwrap();
        }
    });

    let reply = cap_router
        .send_and_await_reply(req, Duration::from_secs(5))
        .await
        .unwrap();

    if let MessagePattern::Response { in_reply_to } = reply.pattern {
        assert_eq!(in_reply_to, req_id);
    } else {
        panic!("Expected Response message pattern");
    }

    if let CapPayload::TaskResult { success, output, .. } = reply.payload {
        assert!(success);
        assert_eq!(output.get("status").and_then(|v| v.as_str()), Some("implemented"));
    } else {
        panic!("Expected TaskResult payload");
    }
}

#[tokio::test]
async fn test_parallel_dag_workflow() {
    let mock = Arc::new(MockModelProvider::new("mock"));
    mock.push_text_response("Design complete.");
    mock.push_text_response("Frontend component created.");
    mock.push_text_response("Backend API route created.");
    mock.push_text_response("All unit tests passed.");

    let mut router = ModelRouter::new();
    router.register(mock);
    let router = Arc::new(router);

    let mut caps = CapabilityRegistry::new();
    register_builtins(&mut caps);
    let caps = Arc::new(caps);

    let event_store = Arc::new(InMemoryEventStore::new());
    let task_store = Arc::new(InMemoryTaskStore::new());
    let runtime_engine = Arc::new(RuntimeEngine::new(router, caps, event_store, task_store));

    let cap_router = Arc::new(CapRouter::new());
    let team = Arc::new(AgentTeam::new(cap_router, runtime_engine));
    team.spawn_default_team().await.unwrap();

    let workflow_engine = WorkflowEngine::new(team);

    // DAG: Step 1 (Architect) -> [Step 2A (Engineer) || Step 2B (Engineer)] -> Step 3 (Reviewer)
    let mut def = WorkflowDef::new("Parallel Build Workflow", "Build frontend and backend in parallel");

    let step1 = StepId::new();
    let step2a = StepId::new();
    let step2b = StepId::new();
    let step3 = StepId::new();

    def.add_step(WorkflowStep {
        step_id: step1,
        name: "Architect Spec".into(),
        description: "Design system".into(),
        assigned_role: AgentRole::Architect,
        prompt: "#plan Design architecture".into(),
        required_tools: vec![],
        retry_policy: StepRetryPolicy::default(),
        timeout_secs: 300,
    });

    def.add_step(WorkflowStep {
        step_id: step2a,
        name: "Frontend Code".into(),
        description: "Write UI".into(),
        assigned_role: AgentRole::Engineer,
        prompt: "#ask Design and draft UI components".into(),
        required_tools: vec![],
        retry_policy: StepRetryPolicy::default(),
        timeout_secs: 300,
    });

    def.add_step(WorkflowStep {
        step_id: step2b,
        name: "Backend Code".into(),
        description: "Write API".into(),
        assigned_role: AgentRole::Engineer,
        prompt: "#ask Design and draft API routes".into(),
        required_tools: vec![],
        retry_policy: StepRetryPolicy::default(),
        timeout_secs: 300,
    });

    def.add_step(WorkflowStep {
        step_id: step3,
        name: "Review & Test".into(),
        description: "Run integration tests".into(),
        assigned_role: AgentRole::Reviewer,
        prompt: "#ask Review overall implementation".into(),
        required_tools: vec![],
        retry_policy: StepRetryPolicy::default(),
        timeout_secs: 300,
    });

    // 1 -> 2a and 1 -> 2b
    def.add_dependency(step1, step2a);
    def.add_dependency(step1, step2b);
    // 2a -> 3 and 2b -> 3 (Join barrier!)
    def.add_dependency(step2a, step3);
    def.add_dependency(step2b, step3);

    let snapshot = workflow_engine.execute(def, None).await.unwrap();

    assert_eq!(snapshot.status, WorkflowStatus::Completed);
    assert_eq!(snapshot.step_outputs.len(), 4, "All 4 DAG steps must complete");
}

#[tokio::test]
async fn test_crash_recovery_checkpoint() {
    let mock = Arc::new(MockModelProvider::new("mock"));
    mock.push_text_response("Plan complete.");
    mock.push_text_response("Step 2 completed.");

    let mut router = ModelRouter::new();
    router.register(mock);
    let router = Arc::new(router);

    let mut caps = CapabilityRegistry::new();
    register_builtins(&mut caps);
    let caps = Arc::new(caps);

    let event_store = Arc::new(InMemoryEventStore::new());
    let task_store = Arc::new(InMemoryTaskStore::new());
    let runtime_engine = Arc::new(RuntimeEngine::new(router, caps, event_store, task_store));

    let cap_router = Arc::new(CapRouter::new());
    let team = Arc::new(AgentTeam::new(cap_router, runtime_engine));
    team.spawn_default_team().await.unwrap();

    let workflow_engine = WorkflowEngine::new(team);

    let mut def = WorkflowDef::new("Recovery Workflow", "Test crash recovery");
    let step1 = StepId::new();
    let step2 = StepId::new();

    def.add_step(WorkflowStep {
        step_id: step1,
        name: "Step 1".into(),
        description: "Initial step".into(),
        assigned_role: AgentRole::Architect,
        prompt: "#ask Initial step".into(),
        required_tools: vec![],
        retry_policy: StepRetryPolicy::default(),
        timeout_secs: 300,
    });

    def.add_step(WorkflowStep {
        step_id: step2,
        name: "Step 2".into(),
        description: "Second step".into(),
        assigned_role: AgentRole::Engineer,
        prompt: "#ask Second step".into(),
        required_tools: vec![],
        retry_policy: StepRetryPolicy::default(),
        timeout_secs: 300,
    });

    def.add_dependency(step1, step2);

    // Simulate partial execution where Step 1 already completed in a previous snapshot
    let mut initial_snapshot = companion_domain::WorkflowStateSnapshot {
        workflow_id: def.workflow_id,
        status: WorkflowStatus::Running,
        step_states: std::collections::HashMap::new(),
        step_outputs: std::collections::HashMap::new(),
        sequence: 1,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    initial_snapshot.step_states.insert(
        step1,
        companion_domain::StepState::Completed {
            output: serde_json::json!({"cached": true}),
            execution_ms: 10,
        },
    );
    initial_snapshot.step_outputs.insert(step1, serde_json::json!({"cached": true}));

    // Resume from the checkpoint: Step 1 should NOT be re-executed, Step 2 should run and finish
    let resumed_snapshot = workflow_engine
        .resume(def, initial_snapshot, None)
        .await
        .unwrap();

    assert_eq!(resumed_snapshot.status, WorkflowStatus::Completed);
    assert_eq!(resumed_snapshot.step_outputs.len(), 2);
    assert_eq!(
        resumed_snapshot.step_outputs.get(&step1),
        Some(&serde_json::json!({"cached": true}))
    );
}

#[tokio::test]
async fn test_cap_message_envelope_routing() {
    let router = CapRouter::new();
    let addr_a = AgentAddress::new(AgentRole::Coordinator);
    let addr_b = AgentAddress::new(AgentRole::Architect);

    let _mb_a = router.register_agent(addr_a.clone(), 10).await;
    let mb_b = router.register_agent(addr_b.clone(), 10).await;

    let envelope = CapEnvelope::new(
        addr_a.clone(),
        addr_b.clone(),
        CorrelationId::new(),
        ConversationId::new(),
        MessagePattern::Request,
        CapPayload::Text {
            content: "Spec query".into(),
        },
    );

    router.route(envelope).await.unwrap();

    let received = mb_b.pop().await.unwrap();
    if let CapPayload::Text { content } = received.payload {
        assert_eq!(content, "Spec query");
    } else {
        panic!("Wrong payload received");
    }
}

#[tokio::test]
async fn test_goal_milestone_progression() {
    let milestone1 = Milestone::new("Design Database", "Schema specified");
    let milestone2 = Milestone::new("Implement Endpoints", "API routes working");

    let m1_id = milestone1.milestone_id;
    let m2_id = milestone2.milestone_id;

    let mut goal = Goal::new(
        TenantId::new(),
        "Build Backend Service",
        "Full implementation of user auth service",
        vec![milestone1, milestone2],
    );

    assert_eq!(goal.status, companion_domain::GoalStatus::Active);
    assert!(!goal.is_all_milestones_completed());

    // Mark milestone 1 completed
    goal.mark_milestone_completed(m1_id, vec![]);
    assert_eq!(goal.status, companion_domain::GoalStatus::Active);

    // Mark milestone 2 completed -> Goal auto-transitions to Completed!
    goal.mark_milestone_completed(m2_id, vec![]);
    assert!(goal.is_all_milestones_completed());
    assert_eq!(goal.status, companion_domain::GoalStatus::Completed);
}

#[tokio::test]
async fn test_cycle_detection_in_dag() {
    let mut def = WorkflowDef::new("Cyclic Workflow", "Should fail compilation");
    let s1 = StepId::new();
    let s2 = StepId::new();

    def.add_step(WorkflowStep {
        step_id: s1,
        name: "A".into(),
        description: "A".into(),
        assigned_role: AgentRole::Architect,
        prompt: "A".into(),
        required_tools: vec![],
        retry_policy: StepRetryPolicy::default(),
        timeout_secs: 300,
    });

    def.add_step(WorkflowStep {
        step_id: s2,
        name: "B".into(),
        description: "B".into(),
        assigned_role: AgentRole::Engineer,
        prompt: "B".into(),
        required_tools: vec![],
        retry_policy: StepRetryPolicy::default(),
        timeout_secs: 300,
    });

    // Cycle: S1 -> S2 and S2 -> S1
    def.add_dependency(s1, s2);
    def.add_dependency(s2, s1);

    let compile_result = companion_workflow::WorkflowDag::compile(def);
    assert!(compile_result.is_err(), "Cycle in DAG must return error");
    let err = compile_result.unwrap_err().to_string();
    assert!(err.contains("Cycle detected"));
}
