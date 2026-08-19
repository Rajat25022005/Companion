use std::sync::Arc;
use companion_context::{ContextBroker, ContextCompiler, SessionManager};
use companion_domain::{
    CapabilityRequirement, ContextBudget, ContextGrant, ContextRequest, ContextSources,
    DataSensitivity, MemoryItem, MemorySearchResult, MemoryTier, Message, Mode,
    ModeProfile, RelationshipTriple, RiskLevel, TaskBudget, TaskContract, TaskId,
    TenantId, TrustClass, WorkspaceId,
};
use companion_memory::{MemoryManager, MockEmbeddingProvider};

fn create_sample_contract() -> TaskContract {
    TaskContract {
        task_id: TaskId::new(),
        tenant_id: TenantId::new(),
        workspace_id: WorkspaceId::new(),
        correlation_id: companion_domain::CorrelationId::new(),
        workflow_id: None,
        goal_id: None,
        parent_task_id: None,
        user_input: "#build Deploy GraphQL API".into(),
        objective: "Deploy GraphQL API with authentication".into(),
        mode_profile: ModeProfile::from_mode(Mode::Build),
        required_capabilities: vec![CapabilityRequirement {
            capability: "filesystem.write".into(),
            required: true,
        }],
        allowed_tools: vec!["filesystem.write".into(), "process.execute".into()],
        completion_conditions: vec![],
        constraints: vec![],
        risk_level: RiskLevel::Medium,
        budget: TaskBudget::default(),
        created_at: chrono::Utc::now(),
    }
}

#[tokio::test]
async fn test_context_compiler_end_to_end() {
    let compiler = ContextCompiler::new();
    let contract = create_sample_contract();

    let memories = vec![
        MemorySearchResult {
            item: MemoryItem::new(
                MemoryTier::Semantic,
                "The project uses Apollo Server for GraphQL.",
            )
            .with_trust_class(TrustClass::UserConfirmed),
            score: 0.95,
            tier: MemoryTier::Semantic,
            match_reasons: vec!["high_similarity".into()],
        },
    ];

    let graph_facts = vec![RelationshipTriple::new("graphql", "runs_on", "port 4000")];

    let sources = ContextSources {
        identity_policy: Some("You are Companion Enterprise Edition.".into()),
        task_contract: Some(contract),
        goal_state: Some("Build phase 1 of backend".into()),
        working_memory: vec!["Generated schema.graphql".into()],
        session_turns: vec![
            Message::user("Hello"),
            Message::assistant("Hello! Ready to build the GraphQL API."),
        ],
        recalled_memories: memories,
        graph_facts,
        selected_tools: vec![],
        artifact_excerpts: vec![("schema.graphql".into(), "type Query { me: User }".into())],
        dependency_outputs: vec![("auth_service".into(), "status=healthy".into())],
        user_input: Some("Please write the resolver code.".into()),
        ..Default::default()
    };

    let budget = ContextBudget::for_total_tokens(4096);
    let compiled = compiler.compile(&sources, &budget, None).await.unwrap();

    assert!(!compiled.messages.is_empty());
    let sys_msg = &compiled.messages[0];
    assert!(sys_msg.content.contains("Companion Enterprise Edition"));
    assert!(sys_msg.content.contains("Deploy GraphQL API with authentication"));
    assert!(sys_msg.content.contains("Apollo Server for GraphQL"));
    assert!(sys_msg.content.contains("port 4000"));
    assert!(sys_msg.content.contains("schema.graphql"));
    assert!(sys_msg.content.contains("auth_service"));

    // Check that user turn message was appended
    let last_msg = compiled.messages.last().unwrap();
    assert_eq!(last_msg.content, "Please write the resolver code.");

    // Check SHA256 fingerprint generated
    assert_eq!(compiled.cache_fingerprint.len(), 64);
    assert!(!compiled.was_truncated);
}

#[tokio::test]
async fn test_priority_token_budget_packing() {
    let compiler = ContextCompiler::new();
    let contract = create_sample_contract();

    // Create a large number of memories
    let mut memories = Vec::new();
    for i in 0..50 {
        memories.push(MemorySearchResult {
            item: MemoryItem::new(
                MemoryTier::Semantic,
                format!("Detailed architectural note {} with lots of background details and instructions.", i),
            )
            .with_importance(1.0),
            score: 0.8,
            tier: MemoryTier::Semantic,
            match_reasons: vec![],
        });
    }

    let sources = ContextSources {
        identity_policy: Some("Base system policy.".into()),
        task_contract: Some(contract),
        recalled_memories: memories,
        user_input: Some("Quick task".into()),
        ..Default::default()
    };

    // Strict tiny budget of 200 tokens
    let budget = ContextBudget::for_total_tokens(200);
    let compiled = compiler.compile(&sources, &budget, None).await.unwrap();

    assert!(compiled.was_truncated);
    // Estimated tokens must be tightly bounded
    assert!(compiled.estimated_tokens <= 250);
}

#[tokio::test]
async fn test_sensitivity_ceiling_filtering() {
    let compiler = ContextCompiler::new();

    let pub_item = MemoryItem::new(MemoryTier::Semantic, "Public API documentation")
        .with_metadata(serde_json::json!({"sensitivity": "public"}));

    let internal_item = MemoryItem::new(MemoryTier::Semantic, "Internal database host is 10.0.0.5")
        .with_metadata(serde_json::json!({"sensitivity": "internal"}));

    let secret_item = MemoryItem::new(MemoryTier::Semantic, "Production API key is secret_xyz")
        .with_metadata(serde_json::json!({"sensitivity": "restricted"}));

    let memories = vec![
        MemorySearchResult {
            item: pub_item,
            score: 0.9,
            tier: MemoryTier::Semantic,
            match_reasons: vec![],
        },
        MemorySearchResult {
            item: internal_item,
            score: 0.85,
            tier: MemoryTier::Semantic,
            match_reasons: vec![],
        },
        MemorySearchResult {
            item: secret_item,
            score: 0.80,
            tier: MemoryTier::Semantic,
            match_reasons: vec![],
        },
    ];

    let sources = ContextSources {
        recalled_memories: memories,
        user_input: Some("Show db and keys".into()),
        ..Default::default()
    };

    // Case 1: Internal Grant (cannot access restricted)
    let internal_grant = ContextGrant::new(TaskId::new(), DataSensitivity::Internal, 2048);
    let compiled_internal = compiler
        .compile(&sources, &ContextBudget::default(), Some(&internal_grant))
        .await
        .unwrap();

    let sys_content = &compiled_internal.messages[0].content;
    assert!(sys_content.contains("Public API documentation"));
    assert!(sys_content.contains("10.0.0.5"));
    assert!(!sys_content.contains("secret_xyz")); // Restricted data filtered!

    // Case 2: Restricted Grant (can access restricted)
    let restricted_grant = ContextGrant::new(TaskId::new(), DataSensitivity::Restricted, 2048);
    let compiled_restricted = compiler
        .compile(&sources, &ContextBudget::default(), Some(&restricted_grant))
        .await
        .unwrap();

    let sys_content_res = &compiled_restricted.messages[0].content;
    assert!(sys_content_res.contains("secret_xyz")); // Now included!
}

#[tokio::test]
async fn test_context_broker_and_grants() {
    let broker = ContextBroker::new();
    let compiler = ContextCompiler::new();
    let task_id = TaskId::new();

    // Issue grant restricting to Semantic tier only
    let mut grant = ContextGrant::new(task_id, DataSensitivity::Internal, 1024);
    grant = grant.with_allowed_tiers(vec![MemoryTier::Semantic]);
    broker.register_grant(grant).await;

    let sources = ContextSources {
        working_memory: vec!["Should be filtered because Working tier not in grant".into()],
        recalled_memories: vec![MemorySearchResult {
            item: MemoryItem::new(MemoryTier::Semantic, "Included semantic memory"),
            score: 0.9,
            tier: MemoryTier::Semantic,
            match_reasons: vec![],
        }],
        graph_facts: vec![RelationshipTriple::new("excluded", "from", "grant")],
        user_input: Some("Test query".into()),
        ..Default::default()
    };

    let request = ContextRequest {
        task_id,
        agent_id: None,
        query: "Test query".into(),
        max_tokens: Some(1024),
        requested_tiers: vec![MemoryTier::Semantic],
    };

    let compiled = broker.request_context(&request, &sources, &compiler).await.unwrap();
    let sys_content = &compiled.messages[0].content;

    assert!(sys_content.contains("Included semantic memory"));
    assert!(!sys_content.contains("Should be filtered"));
    assert!(!sys_content.contains("graph_facts"));
}

#[tokio::test]
async fn test_session_manager_auto_compaction() {
    let embedder = Arc::new(MockEmbeddingProvider::new());
    let memory_manager = Arc::new(MemoryManager::new(embedder));
    let compiler = Arc::new(ContextCompiler::new());
    let session_manager = SessionManager::new(compiler, memory_manager.clone())
        .with_max_session_tokens(100); // Trigger compaction at ~100 tokens

    let session_id = companion_domain::SessionId::new();

    // Simulate multi-turn conversation
    for i in 1..=15 {
        session_manager
            .add_message(
                session_id,
                Message::user(format!("Turn {i}: Explaining feature {i} in detail")),
            )
            .await;
        session_manager
            .add_message(
                session_id,
                Message::assistant(format!("Turn {i}: Acknowledged and implemented feature {i}")),
            )
            .await;
    }

    let budget = ContextBudget::for_total_tokens(1024);
    let compiled = session_manager
        .compile_session_context(&session_id, "Latest status", &budget)
        .await
        .unwrap();

    // Check that session was compacted and compiled context is bounded
    assert!(!compiled.messages.is_empty());
    assert!(compiled.estimated_tokens <= 1024);

    // Verify session store has compacted summary turn
    let msgs = session_manager.session_store().get_messages(&session_id).await;
    assert!(msgs.iter().any(|m| m.content.contains("Compacted Previous Session History")));
}

#[tokio::test]
async fn test_stable_prefix_caching() {
    let compiler = ContextCompiler::new();
    let contract = create_sample_contract();

    let sources = ContextSources {
        identity_policy: Some("Stable system prompt for caching.".into()),
        task_contract: Some(contract),
        user_input: Some("First turn".into()),
        ..Default::default()
    };

    let budget = ContextBudget::default();
    let compiled1 = compiler.compile(&sources, &budget, None).await.unwrap();
    let compiled2 = compiler.compile(&sources, &budget, None).await.unwrap();

    // Fingerprints must be identical
    assert_eq!(compiled1.cache_fingerprint, compiled2.cache_fingerprint);

    // Cache hit count must be recorded
    assert!(compiler.cache().cached_entries_count().await >= 1);
}
