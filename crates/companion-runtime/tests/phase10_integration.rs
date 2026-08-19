use std::sync::Arc;
use std::time::Duration;

use companion_domain::{
    AgentRole, Mode, ModeProfile, PolicyRuleId, RiskLevel, TaskContract, TaskId,
    TaskState, TenantId, WorkspaceId,
};
use companion_events::event::{TaskEvent, TaskEventType};
use companion_policy::{
    ApprovalStatus, DataResidencyGuard, HitlApprovalGate, PolicyCondition,
    PolicyEffect, PolicyEvaluator, PolicyRule,
};
use companion_runtime::{
    CompensationAction, FailureAnalyzer, SelfHealingLoop,
};
use companion_workflow::{PriorityScheduler, ScheduledTask, SwarmCoordinator, WorkerPool};

// ---------------------------------------------------------------------------
// 1. Self-Healing RCA Tests
// ---------------------------------------------------------------------------

#[test]
fn test_failure_analyzer_classifies_rate_limited() {
    let analyzer = FailureAnalyzer::new();
    let contract = TaskContract {
        task_id: TaskId::new(),
        tenant_id: TenantId::new(),
        workspace_id: WorkspaceId::new(),
        correlation_id: companion_domain::CorrelationId::new(),
        workflow_id: None,
        goal_id: None,
        parent_task_id: None,
        user_input: "fetch data".into(),
        objective: "fetch data".into(),
        mode_profile: ModeProfile::from_mode(Mode::Ask),
        required_capabilities: vec![],
        allowed_tools: vec![],
        completion_conditions: vec![],
        constraints: vec![],
        risk_level: RiskLevel::Low,
        budget: Default::default(),
        created_at: chrono::Utc::now(),
    };

    let events = vec![TaskEvent::new(
        contract.task_id,
        contract.correlation_id,
        1,
        TaskEventType::ToolCallFailed,
        serde_json::json!({"error": "HTTP 429 Too Many Requests: Rate limit exceeded"}),
    )];

    let final_state = TaskState::Failed {
        reason: "Rate limit exceeded (HTTP 429)".into(),
    };

    let diagnosis = analyzer.analyze(&contract, &events, &[], &final_state);
    assert_eq!(diagnosis.root_cause.category_label(), "rate_limited");
    assert!(diagnosis.root_cause.is_recoverable());
    assert!(diagnosis.compensation_plan.is_some());

    let plan = diagnosis.compensation_plan.unwrap();
    assert_eq!(plan.actions.len(), 1);
    match &plan.actions[0] {
        CompensationAction::RetryWithBackoff { delay_ms, .. } => {
            assert_eq!(*delay_ms, 2000);
        }
        _ => panic!("Expected RetryWithBackoff action"),
    }
}

#[test]
fn test_failure_analyzer_classifies_permission_denied_as_unrecoverable() {
    let analyzer = FailureAnalyzer::new();
    let contract = TaskContract {
        task_id: TaskId::new(),
        tenant_id: TenantId::new(),
        workspace_id: WorkspaceId::new(),
        correlation_id: companion_domain::CorrelationId::new(),
        workflow_id: None,
        goal_id: None,
        parent_task_id: None,
        user_input: "write sensitive file".into(),
        objective: "write sensitive file".into(),
        mode_profile: ModeProfile::from_mode(Mode::Ask),
        required_capabilities: vec![],
        allowed_tools: vec![],
        completion_conditions: vec![],
        constraints: vec![],
        risk_level: RiskLevel::High,
        budget: Default::default(),
        created_at: chrono::Utc::now(),
    };

    let final_state = TaskState::Failed {
        reason: "Permission denied: Unauthorized access to system directory".into(),
    };

    let diagnosis = analyzer.analyze(&contract, &[], &[], &final_state);
    assert_eq!(diagnosis.root_cause.category_label(), "permission_denied");
    assert!(!diagnosis.root_cause.is_recoverable());
    assert!(diagnosis.compensation_plan.is_none());
}

#[test]
fn test_self_healing_loop_respects_budget() {
    let loop_engine = SelfHealingLoop::new().with_max_attempts(2);
    let contract = TaskContract {
        task_id: TaskId::new(),
        tenant_id: TenantId::new(),
        workspace_id: WorkspaceId::new(),
        correlation_id: companion_domain::CorrelationId::new(),
        workflow_id: None,
        goal_id: None,
        parent_task_id: None,
        user_input: "task".into(),
        objective: "task".into(),
        mode_profile: ModeProfile::from_mode(Mode::Ask),
        required_capabilities: vec![],
        allowed_tools: vec![],
        completion_conditions: vec![],
        constraints: vec![],
        risk_level: RiskLevel::Low,
        budget: Default::default(),
        created_at: chrono::Utc::now(),
    };

    let final_state = TaskState::Failed {
        reason: "Model timeout exceeded".into(),
    };

    // Attempt 0 should produce plan
    let d0 = loop_engine.attempt_healing(&contract, &[], &[], &final_state, 0);
    assert!(d0.is_some());

    // Attempt 1 should produce plan
    let d1 = loop_engine.attempt_healing(&contract, &[], &[], &final_state, 1);
    assert!(d1.is_some());

    // Attempt 2 (budget = 2 exhausted) should return None
    let d2 = loop_engine.attempt_healing(&contract, &[], &[], &final_state, 2);
    assert!(d2.is_none());
}

// ---------------------------------------------------------------------------
// 2. HITL Approval Gate Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_hitl_gate_lifecycle_approve() {
    let gate = HitlApprovalGate::new(Duration::from_secs(3600));
    let task_id = TaskId::new();
    let tenant_id = TenantId::new();

    // 1. Request approval
    let req = gate
        .request_approval(
            task_id,
            tenant_id,
            RiskLevel::Critical,
            "Deploy to production infrastructure".into(),
            vec!["process.execute".into()],
        )
        .await;

    assert_eq!(req.status, ApprovalStatus::Pending);

    // 2. Check pending list
    let pending = gate.list_pending(Some(tenant_id)).await;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].approval_id, req.approval_id);

    // 3. Approve
    let approved = gate
        .approve(req.approval_id, "secops-lead".into())
        .await
        .expect("Approval should succeed");

    match approved.status {
        ApprovalStatus::Approved { approver, .. } => {
            assert_eq!(approver, "secops-lead");
        }
        _ => panic!("Expected Approved status"),
    }

    // 4. Pending list should now be empty
    let pending_after = gate.list_pending(Some(tenant_id)).await;
    assert!(pending_after.is_empty());
}

#[tokio::test]
async fn test_hitl_gate_lifecycle_deny() {
    let gate = HitlApprovalGate::new(Duration::from_secs(3600));
    let task_id = TaskId::new();
    let tenant_id = TenantId::new();

    let req = gate
        .request_approval(
            task_id,
            tenant_id,
            RiskLevel::High,
            "Format disk partition".into(),
            vec!["filesystem.write".into()],
        )
        .await;

    let denied = gate
        .deny(req.approval_id, "Security policy forbids disk format".into())
        .await
        .expect("Deny should succeed");

    match denied.status {
        ApprovalStatus::Denied { reason, .. } => {
            assert_eq!(reason, "Security policy forbids disk format");
        }
        _ => panic!("Expected Denied status"),
    }
}

// ---------------------------------------------------------------------------
// 3. Declarative Policy Engine Tests
// ---------------------------------------------------------------------------

#[test]
fn test_policy_evaluator_evaluates_rules() {
    let mut evaluator = PolicyEvaluator::new();

    // Add rule 1: Deny critical risk tasks without approval
    evaluator.add_rule(PolicyRule {
        rule_id: PolicyRuleId::new(),
        name: "Deny Unsafe Critical Tasks".into(),
        description: "Critical risk tasks are forbidden".into(),
        condition: PolicyCondition::RiskLevelAtLeast {
            level: RiskLevel::Critical,
        },
        effect: PolicyEffect::Deny {
            reason: "Critical tasks require elevated clearance".into(),
        },
        priority: 100,
        active: true,
    });

    // Add rule 2: Require approval for process execution
    evaluator.add_rule(PolicyRule {
        rule_id: PolicyRuleId::new(),
        name: "Require Approval for Process Execution".into(),
        description: "Executing processes requires HITL approval".into(),
        condition: PolicyCondition::CapabilityUsed {
            capability: "process.execute".into(),
        },
        effect: PolicyEffect::RequireApproval { timeout_secs: 1800 },
        priority: 50,
        active: true,
    });

    let mut contract = TaskContract {
        task_id: TaskId::new(),
        tenant_id: TenantId::new(),
        workspace_id: WorkspaceId::new(),
        correlation_id: companion_domain::CorrelationId::new(),
        workflow_id: None,
        goal_id: None,
        parent_task_id: None,
        user_input: "test".into(),
        objective: "test".into(),
        mode_profile: ModeProfile::from_mode(Mode::Ask),
        required_capabilities: vec![],
        allowed_tools: vec!["process.execute".into()],
        completion_conditions: vec![],
        constraints: vec![],
        risk_level: RiskLevel::Medium,
        budget: Default::default(),
        created_at: chrono::Utc::now(),
    };

    // Medium risk + process.execute matches Rule 2
    let decision = evaluator.evaluate(&contract, "process.execute");
    assert_eq!(
        decision.matched_rule.as_deref(),
        Some("Require Approval for Process Execution")
    );
    match decision.effect {
        PolicyEffect::RequireApproval { timeout_secs } => {
            assert_eq!(timeout_secs, 1800);
        }
        _ => panic!("Expected RequireApproval effect"),
    }

    // Critical risk matches Rule 1 (higher priority)
    contract.risk_level = RiskLevel::Critical;
    let decision = evaluator.evaluate(&contract, "process.execute");
    assert_eq!(
        decision.matched_rule.as_deref(),
        Some("Deny Unsafe Critical Tasks")
    );
    match decision.effect {
        PolicyEffect::Deny { reason } => {
            assert!(reason.contains("elevated clearance"));
        }
        _ => panic!("Expected Deny effect"),
    }
}

#[test]
fn test_data_residency_guard() {
    let mut guard = DataResidencyGuard::new();
    let tenant_eu = TenantId::new();
    guard.set_allowed_regions(tenant_eu, vec!["eu-west-1".into(), "eu-central-1".into()]);

    // Allowed region
    assert!(guard.check_residency(&tenant_eu, "eu-west-1").is_ok());

    // Disallowed region
    let err = guard.check_residency(&tenant_eu, "us-east-1").unwrap_err();
    assert!(err.contains("Data residency violation"));
}

// ---------------------------------------------------------------------------
// 4. Swarm Coordinator & Elastic Worker Pool Tests
// ---------------------------------------------------------------------------

#[test]
fn test_swarm_coordinator_goal_decomposition() {
    let pool = Arc::new(WorkerPool::new(5));
    let scheduler = Arc::new(PriorityScheduler::new());
    let coordinator = SwarmCoordinator::new(pool, scheduler);

    let tenant_id = TenantId::new();
    let available_roles = vec![
        "architect".to_string(),
        "engineer".to_string(),
        "reviewer".to_string(),
    ];

    let workflow = coordinator
        .decompose_goal(
            "Build a distributed cache with LRU eviction and replication",
            tenant_id,
            &available_roles,
        )
        .expect("Decomposition should succeed");

    assert_eq!(workflow.steps.len(), 3);
    assert_eq!(workflow.dependencies.len(), 2);
    assert_eq!(workflow.steps[0].assigned_role, AgentRole::Architect);
    assert_eq!(workflow.steps[1].assigned_role, AgentRole::Engineer);
    assert_eq!(workflow.steps[2].assigned_role, AgentRole::Reviewer);
}

#[tokio::test]
async fn test_priority_scheduler_weighted_fair_share() {
    let scheduler = PriorityScheduler::new();

    let tenant_vip = TenantId::new();
    let tenant_free = TenantId::new();

    // Set VIP weight 3.0, free weight 1.0
    scheduler.set_tenant_weight(tenant_vip, 3.0).await;
    scheduler.set_tenant_weight(tenant_free, 1.0).await;

    // Submit standard priority 10 tasks for both
    let task_free = ScheduledTask {
        task_id: TaskId::new(),
        tenant_id: tenant_free,
        priority: 10,
        submitted_at: chrono::Utc::now(),
        prompt: "free task".into(),
        effective_priority: 0.0,
    };

    let task_vip = ScheduledTask {
        task_id: TaskId::new(),
        tenant_id: tenant_vip,
        priority: 10,
        submitted_at: chrono::Utc::now(),
        prompt: "vip task".into(),
        effective_priority: 0.0,
    };

    scheduler.enqueue(task_free).await;
    scheduler.enqueue(task_vip).await;

    assert_eq!(scheduler.queue_depth().await, 2);

    // VIP should dequeue first because 10 * 3.0 = 30.0 > 10 * 1.0 = 10.0
    let first = scheduler.dequeue().await.unwrap();
    assert_eq!(first.tenant_id, tenant_vip);

    let second = scheduler.dequeue().await.unwrap();
    assert_eq!(second.tenant_id, tenant_free);
}

#[tokio::test]
async fn test_worker_pool_concurrency_bounding() {
    let pool = WorkerPool::new(2);
    assert_eq!(pool.max_concurrency(), 2);
    assert_eq!(pool.active_count().await, 0);

    let t1 = TaskId::new();
    let t2 = TaskId::new();

    let permit1 = pool.acquire(t1).await.expect("Permit 1 should succeed");
    assert_eq!(pool.active_count().await, 1);

    let permit2 = pool.acquire(t2).await.expect("Permit 2 should succeed");
    assert_eq!(pool.active_count().await, 2);

    // Release t1
    drop(permit1);
    pool.release(t1).await;
    assert_eq!(pool.active_count().await, 1);
    assert_eq!(pool.completed_count().await, 1);

    // Release t2
    drop(permit2);
    pool.release(t2).await;
    assert_eq!(pool.active_count().await, 0);
    assert_eq!(pool.completed_count().await, 2);
}
