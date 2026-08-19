use std::sync::Arc;
use tempfile::tempdir;

use companion_capabilities::{builtins::register_builtins, CapabilityRegistry};
use companion_domain::{ModelError, TaskState, ToolCall};
use companion_events::EventStore;
use companion_models::{MockModelProvider, ModelRouter};
use companion_runtime::RuntimeEngine;
use companion_storage::{InMemoryEventStore, InMemoryTaskStore};

#[tokio::test]
async fn test_ask_no_tools() {
    let mock = Arc::new(MockModelProvider::new("mock"));
    mock.push_text_response("Rust is a systems programming language focused on safety and speed.");

    let mut router = ModelRouter::new();
    router.register(mock);
    let router = Arc::new(router);

    let mut caps = CapabilityRegistry::new();
    register_builtins(&mut caps);
    let caps = Arc::new(caps);

    let event_store = Arc::new(InMemoryEventStore::new());
    let task_store = Arc::new(InMemoryTaskStore::new());

    let engine = RuntimeEngine::new(router, caps, event_store.clone(), task_store);
    let (task_id, state, _contract) = engine
        .submit_and_run("#ask What is Rust?", None, None, None)
        .await
        .unwrap();

    assert_eq!(state, TaskState::Completed);

    let events = event_store.load_events(task_id).await.unwrap();
    assert!(!events.is_empty());
}

#[tokio::test]
async fn test_build_creates_file() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("hello.txt").to_str().unwrap().to_string();

    let mock = Arc::new(MockModelProvider::new("mock"));
    // Turn 1: model proposes tool call to write file
    mock.push_tool_call_response(vec![ToolCall {
        id: "call_1".into(),
        name: "filesystem.write".into(),
        arguments: serde_json::json!({
            "path": file_path,
            "content": "Hello World from Rust Kernel!"
        }),
    }]);
    // Turn 2: model finishes
    mock.push_text_response("Created hello.txt with content.");

    let mut router = ModelRouter::new();
    router.register(mock);
    let router = Arc::new(router);

    let mut caps = CapabilityRegistry::new();
    register_builtins(&mut caps);
    let caps = Arc::new(caps);

    let event_store = Arc::new(InMemoryEventStore::new());
    let task_store = Arc::new(InMemoryTaskStore::new());

    let engine = RuntimeEngine::new(router, caps, event_store.clone(), task_store);
    let input = format!("#build Create {} with content", file_path);
    let (task_id, state, _contract) = engine
        .submit_and_run(&input, None, None, None)
        .await
        .unwrap();

    assert_eq!(state, TaskState::Completed);

    // Verify file actually exists on disk!
    let content = tokio::fs::read_to_string(&file_path).await.unwrap();
    assert_eq!(content, "Hello World from Rust Kernel!");

    // Verify events
    let events = event_store.load_events(task_id).await.unwrap();
    let event_types: Vec<_> = events.iter().map(|e| e.event_type.to_string()).collect();
    assert!(event_types.contains(&"tool_call_started".to_string()));
    assert!(event_types.contains(&"tool_call_completed".to_string()));
    assert!(event_types.contains(&"verification_passed".to_string()));
    assert!(event_types.contains(&"task_completed".to_string()));
}

#[tokio::test]
async fn test_required_tool_enforcement() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("enforce.txt").to_str().unwrap().to_string();

    let mock = Arc::new(MockModelProvider::new("mock"));
    // Turn 1: model tries to cheat by outputting text only without calling tools for #build
    mock.push_text_response("I have completed the task without tools.");
    // Turn 2: after policy rejection, model calls the required tool
    mock.push_tool_call_response(vec![ToolCall {
        id: "call_1".into(),
        name: "filesystem.write".into(),
        arguments: serde_json::json!({
            "path": file_path,
            "content": "Real file content"
        }),
    }]);
    // Turn 3: finish
    mock.push_text_response("File created.");

    let mut router = ModelRouter::new();
    router.register(mock);
    let router = Arc::new(router);

    let mut caps = CapabilityRegistry::new();
    register_builtins(&mut caps);
    let caps = Arc::new(caps);

    let event_store = Arc::new(InMemoryEventStore::new());
    let task_store = Arc::new(InMemoryTaskStore::new());

    let engine = RuntimeEngine::new(router, caps, event_store.clone(), task_store);
    let input = format!("#build Create {} with content", file_path);
    let (task_id, state, _contract) = engine
        .submit_and_run(&input, None, None, None)
        .await
        .unwrap();

    assert_eq!(state, TaskState::Completed);

    let events = event_store.load_events(task_id).await.unwrap();
    let rejected_count = events
        .iter()
        .filter(|e| e.event_type == companion_events::TaskEventType::TurnRejected)
        .count();
    assert_eq!(rejected_count, 1, "ToolIntentMonitor should have rejected turn 1");
}

#[tokio::test]
async fn test_state_transitions_persisted() {
    let mock = Arc::new(MockModelProvider::new("mock"));
    mock.push_text_response("Done.");

    let mut router = ModelRouter::new();
    router.register(mock);
    let router = Arc::new(router);

    let mut caps = CapabilityRegistry::new();
    register_builtins(&mut caps);
    let caps = Arc::new(caps);

    let event_store = Arc::new(InMemoryEventStore::new());
    let task_store = Arc::new(InMemoryTaskStore::new());

    let engine = RuntimeEngine::new(router, caps, event_store.clone(), task_store);
    let (task_id, state, _contract) = engine
        .submit_and_run("#ask Quick question", None, None, None)
        .await
        .unwrap();

    assert_eq!(state, TaskState::Completed);

    let events = event_store.load_events(task_id).await.unwrap();
    let transition_events: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == companion_events::TaskEventType::StateTransition)
        .collect();

    // Must have recorded transitions
    assert!(transition_events.len() >= 4);
}

#[tokio::test]
async fn test_verification_failure_triggers_repair() {
    let dir = tempdir().unwrap();
    let wrong_path = dir.path().join("wrong.txt").to_str().unwrap().to_string();
    let correct_path = dir.path().join("target.txt").to_str().unwrap().to_string();

    let mock = Arc::new(MockModelProvider::new("mock"));
    // Turn 1: model writes wrong file
    mock.push_tool_call_response(vec![ToolCall {
        id: "call_1".into(),
        name: "filesystem.write".into(),
        arguments: serde_json::json!({
            "path": wrong_path,
            "content": "wrong"
        }),
    }]);
    mock.push_text_response("Done.");

    // Turn 2: after verification fails, model writes correct file during Repairing
    mock.push_tool_call_response(vec![ToolCall {
        id: "call_2".into(),
        name: "filesystem.write".into(),
        arguments: serde_json::json!({
            "path": correct_path,
            "content": "correct"
        }),
    }]);
    mock.push_text_response("Corrected.");

    let mut router = ModelRouter::new();
    router.register(mock);
    let router = Arc::new(router);

    let mut caps = CapabilityRegistry::new();
    register_builtins(&mut caps);
    let caps = Arc::new(caps);

    let event_store = Arc::new(InMemoryEventStore::new());
    let task_store = Arc::new(InMemoryTaskStore::new());

    let engine = RuntimeEngine::new(router, caps, event_store.clone(), task_store);
    let input = format!("#build Create {} with content", correct_path);
    let (task_id, state, _contract) = engine
        .submit_and_run(&input, None, None, None)
        .await
        .unwrap();

    assert_eq!(state, TaskState::Completed);

    let events = event_store.load_events(task_id).await.unwrap();
    let repair_events: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == companion_events::TaskEventType::VerificationFailed)
        .collect();
    assert!(!repair_events.is_empty(), "Should have recorded verification failure before repair");
}

#[tokio::test]
async fn test_budget_exceeded() {
    let mock = Arc::new(MockModelProvider::new("mock"));
    // Always return tool calls to simulate an infinite loop
    for i in 0..25 {
        mock.push_tool_call_response(vec![ToolCall {
            id: format!("call_{i}"),
            name: "filesystem.list".into(),
            arguments: serde_json::json!({"path": "."}),
        }]);
    }

    let mut router = ModelRouter::new();
    router.register(mock);
    let router = Arc::new(router);

    let mut caps = CapabilityRegistry::new();
    register_builtins(&mut caps);
    let caps = Arc::new(caps);

    let event_store = Arc::new(InMemoryEventStore::new());
    let task_store = Arc::new(InMemoryTaskStore::new());

    let engine = RuntimeEngine::new(router, caps, event_store.clone(), task_store);
    let (task_id, state, _contract) = engine
        .submit_and_run("#build Create something in loop", None, None, None)
        .await
        .unwrap();

    assert!(matches!(state, TaskState::Failed { .. }));

    let events = event_store.load_events(task_id).await.unwrap();
    let failed_event = events
        .iter()
        .find(|e| e.event_type == companion_events::TaskEventType::TaskFailed);
    assert!(failed_event.is_some());
}

#[tokio::test]
async fn test_provider_retry() {
    let mock = Arc::new(MockModelProvider::new("mock"));
    // First attempt fails with transient rate limit / network error
    mock.push_error(ModelError::RateLimited { retry_after_secs: 1 });
    // Second attempt succeeds
    mock.push_text_response("Recovered after retry.");

    let mut router = ModelRouter::new();
    router.register(mock);
    let router = Arc::new(router);

    let mut caps = CapabilityRegistry::new();
    register_builtins(&mut caps);
    let caps = Arc::new(caps);

    let event_store = Arc::new(InMemoryEventStore::new());
    let task_store = Arc::new(InMemoryTaskStore::new());

    let engine = RuntimeEngine::new(router, caps, event_store.clone(), task_store);
    let (_task_id, state, _contract) = engine
        .submit_and_run("#ask Question with transient failure", None, None, None)
        .await
        .unwrap();

    assert_eq!(state, TaskState::Completed);
}

#[tokio::test]
async fn test_unauthorized_tool_rejected() {
    let mock = Arc::new(MockModelProvider::new("mock"));
    // Model proposes an unauthorized tool call (e.g., process.execute on #ask mode)
    mock.push_tool_call_response(vec![ToolCall {
        id: "call_unauth".into(),
        name: "process.execute".into(),
        arguments: serde_json::json!({"command": "rm -rf /"}),
    }]);
    mock.push_text_response("I see process execution is unauthorized for questions.");

    let mut router = ModelRouter::new();
    router.register(mock);
    let router = Arc::new(router);

    let mut caps = CapabilityRegistry::new();
    register_builtins(&mut caps);
    let caps = Arc::new(caps);

    let event_store = Arc::new(InMemoryEventStore::new());
    let task_store = Arc::new(InMemoryTaskStore::new());

    let engine = RuntimeEngine::new(router, caps, event_store.clone(), task_store);
    let (task_id, state, _contract) = engine
        .submit_and_run("#ask What is my disk usage?", None, None, None)
        .await
        .unwrap();

    assert_eq!(state, TaskState::Completed);

    let events = event_store.load_events(task_id).await.unwrap();
    let auth_denied = events.iter().any(|e| {
        e.event_type == companion_events::TaskEventType::AuthorizationDecision
            && e.payload.get("authorized") == Some(&serde_json::json!(false))
    });
    assert!(auth_denied, "Should record unauthorized tool denial");
}

#[tokio::test]
async fn test_task_execution_with_context_and_memory() {
    let mock = Arc::new(MockModelProvider::new("mock"));
    mock.push_text_response("Configured RSA-256 JWT auth keys successfully.");

    let mut router = ModelRouter::new();
    router.register(mock);
    let router = Arc::new(router);

    let mut caps = CapabilityRegistry::new();
    register_builtins(&mut caps);
    let caps = Arc::new(caps);

    let event_store = Arc::new(InMemoryEventStore::new());
    let task_store = Arc::new(InMemoryTaskStore::new());

    let embedder = Arc::new(companion_memory::MockEmbeddingProvider::new());
    let memory_manager = Arc::new(companion_memory::MemoryManager::new(embedder));
    let context_compiler = Arc::new(companion_context::ContextCompiler::new());

    // Pre-populate memory
    memory_manager
        .remember(
            "Project auth uses RSA-256 JWT keys.",
            companion_domain::MemoryTier::Semantic,
            1.2,
        )
        .await
        .unwrap();

    let engine = RuntimeEngine::new(router, caps, event_store.clone(), task_store)
        .with_context_compiler(context_compiler)
        .with_memory_manager(memory_manager.clone());

    let (task_id, state, _contract) = engine
        .submit_and_run("#ask Configure auth JWT keys", None, None, None)
        .await
        .unwrap();

    assert_eq!(state, TaskState::Completed);

    // Verify episodic memory was recorded
    let recalled_episodes = memory_manager
        .recall("Configure auth JWT keys", 2, 0.0)
        .await
        .unwrap();
    assert!(!recalled_episodes.is_empty());
}

#[tokio::test]
async fn test_task_execution_with_skill_injection() {
    let mock = Arc::new(MockModelProvider::new("mock"));
    mock.push_text_response("Executed database migration following skill procedure.");

    let mut router = ModelRouter::new();
    router.register(mock);
    let router = Arc::new(router);

    let mut caps = CapabilityRegistry::new();
    register_builtins(&mut caps);
    let caps = Arc::new(caps);

    let event_store = Arc::new(InMemoryEventStore::new());
    let task_store = Arc::new(InMemoryTaskStore::new());

    let embedder = Arc::new(companion_memory::MockEmbeddingProvider::new());
    let memory_manager = Arc::new(companion_memory::MemoryManager::new(embedder));
    let context_compiler = Arc::new(companion_context::ContextCompiler::new());
    let skill_registry = Arc::new(companion_skills::SkillRegistry::new());

    // Register active procedural skill
    let step1 = companion_domain::ProcedureStep::new("step_1", "Validate schema", "Run validation");
    let step2 = companion_domain::ProcedureStep::new("step_2", "Apply migration", "Apply SQL migrations");
    let skill = companion_domain::Skill::new("db-migrate", 1, "Standard database migration workflow")
        .with_steps(vec![step1, step2])
        .with_triggers(vec![companion_domain::SkillTrigger {
            intent: Some("migrate database".into()),
            keywords: vec!["migration".into(), "database".into()],
            required_capabilities: vec![],
            mode: None,
        }]);

    skill_registry.register_active(skill).await;

    let engine = RuntimeEngine::new(router, caps, event_store.clone(), task_store)
        .with_context_compiler(context_compiler)
        .with_memory_manager(memory_manager)
        .with_skill_registry(skill_registry.clone());

    let (task_id, state, _contract) = engine
        .submit_and_run("#ask Run database migration for v2", None, None, None)
        .await
        .unwrap();

    assert_eq!(state, TaskState::Completed);

    let events = event_store.load_events(task_id).await.unwrap();
    assert!(!events.is_empty());
}
