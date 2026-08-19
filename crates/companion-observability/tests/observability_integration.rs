use companion_domain::{TaskId, TenantId};
use companion_observability::{AuditLedger, MetricsCollector};

#[tokio::test]
async fn test_metrics_collector_aggregation_and_prometheus_export() {
    let metrics = MetricsCollector::new();

    // 1. Record tasks
    metrics.record_task(true, false);
    metrics.record_task(true, true);
    metrics.record_task(false, false);

    // 2. Record tool invocations
    metrics.record_tool_call("filesystem.write", 45, false).await;
    metrics.record_tool_call("filesystem.read", 12, false).await;
    metrics.record_tool_call("process.execute", 250, true).await;

    // 3. Record token accounting & context caching
    metrics.record_tokens(1500, 450, 0.0125);
    metrics.record_context_compilation(true);
    metrics.record_context_compilation(false);
    metrics.record_skill_execution(false);
    metrics.record_skill_execution(true);

    // 4. Verify snapshot
    let snap = metrics.snapshot().await;
    assert_eq!(snap.tasks_total, 3);
    assert_eq!(snap.tasks_succeeded, 2);
    assert_eq!(snap.tasks_failed, 1);
    assert_eq!(snap.tasks_repaired, 1);
    assert_eq!(snap.tool_calls_total, 3);
    assert_eq!(snap.tool_errors_total, 1);
    assert_eq!(snap.prompt_tokens_total, 1500);
    assert_eq!(snap.completion_tokens_total, 450);
    assert!(snap.estimated_cost_usd > 0.012);
    assert_eq!(snap.context_compilations_total, 2);
    assert_eq!(snap.context_cache_hits_total, 1);
    assert_eq!(snap.skill_executions_total, 2);
    assert_eq!(snap.skill_rollbacks_total, 1);

    // 5. Verify Prometheus output
    let prom = metrics.export_prometheus().await;
    assert!(prom.contains("companion_tasks_total 3"));
    assert!(prom.contains("companion_tasks_succeeded 2"));
    assert!(prom.contains("companion_tasks_failed 1"));
    assert!(prom.contains("companion_tool_calls_total 3"));
    assert!(prom.contains("companion_tool_errors_total 1"));
    assert!(prom.contains("companion_tool_invocations_total{tool=\"filesystem.write\"} 1"));
    assert!(prom.contains("companion_tool_invocations_total{tool=\"process.execute\"} 1"));
}

#[tokio::test]
async fn test_cryptographic_audit_ledger_hash_chain_and_tamper_detection() {
    let ledger = AuditLedger::new();
    let tenant_id = TenantId::new();
    let task_id = TaskId::new();

    // 1. Append sequential audit entries
    let entry1 = ledger.append(
        tenant_id,
        Some(task_id),
        "user:operator",
        "authz.grant",
        serde_json::json!({"scope": "workspace:write"}),
    ).await;
    assert_eq!(entry1.sequence, 1);
    assert_eq!(entry1.prev_hash, "0000000000000000000000000000000000000000000000000000000000000000");

    let entry2 = ledger.append(
        tenant_id,
        Some(task_id),
        "runtime:engine",
        "tool.execute",
        serde_json::json!({"tool": "filesystem.write", "path": "src/main.rs"}),
    ).await;
    assert_eq!(entry2.sequence, 2);
    assert_eq!(entry2.prev_hash, entry1.entry_hash);

    let entry3 = ledger.append(
        tenant_id,
        None,
        "operator",
        "skill.promote",
        serde_json::json!({"skill": "rust_refactor", "version": 2}),
    ).await;
    assert_eq!(entry3.sequence, 3);
    assert_eq!(entry3.prev_hash, entry2.entry_hash);

    // 2. Verify intact chain
    let verification = ledger.verify_integrity().await;
    assert!(verification.is_ok());
    assert!(verification.unwrap());

    // 3. Verify entry count and pagination
    assert_eq!(ledger.count().await, 3);
    let recent = ledger.get_entries(2).await;
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].sequence, 3);
    assert_eq!(recent[1].sequence, 2);
}
