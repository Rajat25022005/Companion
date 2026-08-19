use std::sync::Arc;
use companion_domain::{
    CapabilityRequirement, CorrelationId, Mode, ModeProfile,
    RiskLevel, TaskBudget, TaskContract, TaskId, TaskState, TenantId, WorkspaceId,
    MemoryTier,
};
use companion_events::{TaskEvent, TaskEventType};
use companion_memory::{
    EmbeddingProvider, MemoryManager, MockEmbeddingProvider, VectorStore,
};

#[tokio::test]
async fn test_vector_similarity_search() {
    let embedder = Arc::new(MockEmbeddingProvider::new());
    let manager = MemoryManager::new(embedder);

    manager
        .remember(
            "Rust provides fearless concurrency with Send and Sync traits.",
            MemoryTier::Semantic,
            1.0,
        )
        .await
        .unwrap();

    manager
        .remember(
            "Python uses a Global Interpreter Lock for thread synchronization.",
            MemoryTier::Semantic,
            1.0,
        )
        .await
        .unwrap();

    manager
        .remember(
            "PostgreSQL is an ACID compliant relational database.",
            MemoryTier::Semantic,
            1.0,
        )
        .await
        .unwrap();

    let results = manager
        .recall("Rust concurrency thread safety", 2, 0.0)
        .await
        .unwrap();

    assert!(!results.is_empty());
    assert!(results[0].item.content.contains("Rust provides fearless concurrency"));
}

#[tokio::test]
async fn test_knowledge_graph_triples() {
    let embedder = Arc::new(MockEmbeddingProvider::new());
    let manager = MemoryManager::new(embedder);

    manager.add_fact("Rust", "has_feature", "Ownership").await;
    manager.add_fact("Ownership", "guarantees", "Memory Safety").await;
    manager.add_fact("Memory Safety", "prevents", "Buffer Overflows").await;

    // 1-hop query from Rust
    let facts_1hop = manager.query_graph("Rust", 1).await;
    assert_eq!(facts_1hop.len(), 1);
    assert_eq!(facts_1hop[0].subject, "rust");
    assert_eq!(facts_1hop[0].predicate, "has_feature");
    assert_eq!(facts_1hop[0].object, "ownership");

    // 3-hop query from Rust
    let facts_3hop = manager.query_graph("Rust", 3).await;
    assert_eq!(facts_3hop.len(), 3);
    let objects: Vec<_> = facts_3hop.iter().map(|f| f.object.as_str()).collect();
    assert!(objects.contains(&"ownership"));
    assert!(objects.contains(&"memory safety"));
    assert!(objects.contains(&"buffer overflows"));
}

#[tokio::test]
async fn test_episodic_memory_creation() {
    let embedder = Arc::new(MockEmbeddingProvider::new());
    let manager = MemoryManager::new(embedder);

    let contract = TaskContract {
        task_id: TaskId::new(),
        tenant_id: TenantId::new(),
        workspace_id: WorkspaceId::new(),
        correlation_id: CorrelationId::new(),
        workflow_id: None,
        goal_id: None,
        parent_task_id: None,
        user_input: "#build Create auth middleware".into(),
        objective: "Create auth middleware".into(),
        mode_profile: ModeProfile::from_mode(Mode::Build),
        required_capabilities: vec![CapabilityRequirement {
            capability: "filesystem.write".into(),
            required: true,
        }],
        allowed_tools: vec!["filesystem.write".into()],
        completion_conditions: vec![],
        constraints: vec![],
        risk_level: RiskLevel::Medium,
        budget: TaskBudget::default(),
        created_at: chrono::Utc::now(),
    };

    let events = vec![
        TaskEvent::new(
            contract.task_id,
            contract.correlation_id,
            1,
            TaskEventType::TaskCreated,
            serde_json::json!({}),
        ),
        TaskEvent::new(
            contract.task_id,
            contract.correlation_id,
            2,
            TaskEventType::ToolCallStarted,
            serde_json::json!({"name": "filesystem.write"}),
        ),
    ];

    let item = manager
        .episodic_recorder()
        .record_task_episode(&contract, &TaskState::Completed, &events)
        .await
        .unwrap();

    assert_eq!(item.tier, MemoryTier::Episodic);
    assert!(item.content.contains("Create auth middleware"));

    // Verify recall finds this episode
    let recalled = manager.recall("auth middleware", 1, 0.0).await.unwrap();
    assert!(!recalled.is_empty());
    assert_eq!(recalled[0].item.memory_id, item.memory_id);
}

#[tokio::test]
async fn test_hybrid_memory_recall() {
    let embedder = Arc::new(MockEmbeddingProvider::new());
    let manager = MemoryManager::new(embedder);

    // Add semantic memory
    manager
        .remember(
            "JWT tokens are used for stateless API authentication.",
            MemoryTier::Semantic,
            1.0,
        )
        .await
        .unwrap();

    // Add graph facts
    manager.add_fact("JWT", "contains", "Claims").await;
    manager.add_fact("Claims", "includes", "Expiration Time").await;

    // Assemble unified context
    let context = manager.assemble_context("JWT authentication", 500).await.unwrap();

    assert!(context.contains("Relevant Memories"));
    assert!(context.contains("JWT tokens are used for stateless API authentication"));
    assert!(context.contains("Knowledge Graph Context"));
    assert!(context.contains("jwt"));
}

#[tokio::test]
async fn test_context_assembler_budget() {
    let embedder = Arc::new(MockEmbeddingProvider::new());
    let manager = MemoryManager::new(embedder);

    for i in 0..10 {
        manager
            .remember(
                &format!("Detailed memory entry number {} with extensive background explanations.", i),
                MemoryTier::Semantic,
                1.0,
            )
            .await
            .unwrap();
    }

    // Request very small token budget (e.g. 10 tokens ~ 40 chars)
    let context = manager.assemble_context("entry", 10).await.unwrap();
    assert!(context.contains("Truncated for context budget"));
    assert!(context.len() < 120);
}

#[tokio::test]
async fn test_memory_decay_and_importance() {
    let embedder = Arc::new(MockEmbeddingProvider::new());
    let store = VectorStore::new();

    let emb = embedder.embed("General system note").await.unwrap();

    let low_item = companion_domain::MemoryItem::new(MemoryTier::Semantic, "General system note")
        .with_embedding(emb.clone())
        .with_importance(0.5);

    let high_item = companion_domain::MemoryItem::new(MemoryTier::Semantic, "General system note")
        .with_embedding(emb.clone())
        .with_importance(2.0);

    store.insert(low_item.clone()).await;
    store.insert(high_item.clone()).await;

    let results = store.search(&emb, 2, 0.0, None).await;
    assert_eq!(results.len(), 2);
    // Highest importance item must be ranked first
    assert_eq!(results[0].item.memory_id, high_item.memory_id);
    assert!(results[0].score > results[1].score);
}

#[tokio::test]
async fn test_working_memory_scratchpad() {
    let embedder = Arc::new(MockEmbeddingProvider::new());
    let manager = MemoryManager::new(embedder);
    let task_id = TaskId::new();

    manager
        .working_memory()
        .push_scratchpad(task_id, "Investigating postgres connection timeout")
        .await;
    manager
        .working_memory()
        .push_scratchpad(task_id, "Found port 5432 is blocked by firewall")
        .await;

    let notes = manager.working_memory().get_scratchpad(&task_id).await;
    assert_eq!(notes.len(), 2);
    assert_eq!(notes[0].tier, MemoryTier::Working);

    let formatted = manager
        .working_memory()
        .format_working_notes(&task_id)
        .await
        .unwrap();
    assert!(formatted.contains("Working Memory (Scratchpad)"));
    assert!(formatted.contains("port 5432"));

    // Clear task
    manager.working_memory().clear_task(&task_id).await;
    let cleared = manager.working_memory().get_scratchpad(&task_id).await;
    assert!(cleared.is_empty());
}

#[tokio::test]
async fn test_session_store_sliding_window_and_compaction() {
    let embedder = Arc::new(MockEmbeddingProvider::new());
    let manager = MemoryManager::new(embedder);
    let session_id = companion_domain::SessionId::new();

    for i in 1..=10 {
        manager
            .session_store()
            .append_message(
                session_id,
                companion_domain::Message::user(format!("User message {i}")),
            )
            .await;
        manager
            .session_store()
            .append_message(
                session_id,
                companion_domain::Message::assistant(format!("Assistant response {i}")),
            )
            .await;
    }

    // 20 messages total
    let total_msgs = manager.session_store().get_messages(&session_id).await;
    assert_eq!(total_msgs.len(), 20);

    // Sliding window: last 4 messages
    let recent = manager.session_store().get_recent_turns(&session_id, 4).await;
    assert_eq!(recent.len(), 4);
    assert_eq!(recent[3].content, "Assistant response 10");

    // Compact session: threshold 10, keep recent 4
    let summary_item = manager
        .session_store()
        .compact_session(&session_id, 10, 4)
        .await;
    assert!(summary_item.is_some());
    let item = summary_item.unwrap();
    assert_eq!(item.tier, MemoryTier::Session);
    assert!(item.content.contains("Compacted Previous Session History"));

    // Verify session now has 1 summary message + 4 recent turns = 5 total
    let compacted_msgs = manager.session_store().get_messages(&session_id).await;
    assert_eq!(compacted_msgs.len(), 5);
}

#[tokio::test]
async fn test_trust_class_precedence_and_contradiction_superseding() {
    let embedder = Arc::new(MockEmbeddingProvider::new());
    let manager = MemoryManager::new(embedder);

    let low_trust_item = companion_domain::MemoryItem::new(
        MemoryTier::Semantic,
        "The server is deployed on AWS EC2.",
    )
    .with_subject("deployment_server")
    .with_trust_class(companion_domain::TrustClass::AgentInferred);

    let high_trust_item = companion_domain::MemoryItem::new(
        MemoryTier::Semantic,
        "The server is deployed on Google Cloud Run.",
    )
    .with_subject("deployment_server")
    .with_trust_class(companion_domain::TrustClass::UserConfirmed);

    let low_saved = manager.remember_record(low_trust_item).await.unwrap();
    let high_saved = manager.remember_record(high_trust_item).await.unwrap();

    // Trigger consolidation to resolve contradictions
    let report = manager.consolidate(&[], &[]).await.unwrap();
    assert!(report.contradictions_resolved >= 1);
    assert!(report.records_superseded >= 1);

    // Low trust item must now be superseded
    let retrieved_low = manager.vector_store().get(&low_saved.memory_id).await.unwrap();
    assert_eq!(retrieved_low.status, companion_domain::MemoryStatus::Superseded);
    assert!(retrieved_low.supersedes.contains(&high_saved.memory_id));

    // High trust item remains Active
    let retrieved_high = manager.vector_store().get(&high_saved.memory_id).await.unwrap();
    assert_eq!(retrieved_high.status, companion_domain::MemoryStatus::Active);
}

#[tokio::test]
async fn test_dream_cycle_consolidation() {
    let embedder = Arc::new(MockEmbeddingProvider::new());
    let manager = MemoryManager::new(embedder);

    let task_id = TaskId::new();
    let corr_id = CorrelationId::new();

    let events = vec![
        TaskEvent::new(
            task_id,
            corr_id,
            1,
            TaskEventType::ToolCallCompleted,
            serde_json::json!({
                "name": "cargo.build",
                "result": "exit_code=0 compilation succeeded",
                "target": "binary_target"
            }),
        ),
        TaskEvent::new(
            task_id,
            corr_id,
            2,
            TaskEventType::TaskCompleted,
            serde_json::json!({
                "objective": "Build and test the Rust microservice"
            }),
        ),
    ];

    let episodes = vec![
        companion_domain::MemoryItem::new(
            MemoryTier::Episodic,
            "Task: Deploy auth service\nObjective: Configure JWT keys\nOutcome: completed",
        )
        .with_trust_class(companion_domain::TrustClass::ToolVerified),
    ];

    let report = manager.consolidate(&episodes, &events).await.unwrap();
    assert_eq!(report.episodes_processed, 1);
    assert!(report.facts_extracted >= 3);
    assert!(report.triples_created >= 1);

    // Verify graph has triple: cargo.build -> produced -> binary_target
    let facts = manager.query_graph("cargo.build", 1).await;
    assert!(!facts.is_empty());
    assert_eq!(facts[0].subject, "cargo.build");
    assert_eq!(facts[0].predicate, "produced");
    assert_eq!(facts[0].object, "binary_target");
}
