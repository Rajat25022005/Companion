use std::convert::Infallible;
use std::sync::Arc;
use std::time::Instant;
use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use futures_util::stream::{self, Stream};
use serde::Deserialize;
use tokio_stream::StreamExt;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;
use uuid::Uuid;

use companion_agents::AgentTeam;
use companion_cap::CapRouter;
use companion_capabilities::{builtins::register_builtins, CapabilityRegistry};
use companion_domain::{ApprovalId, TaskId, TenantId, WorkflowDef, WorkflowId};
use companion_events::event::TaskEventType;
use companion_models::{GeminiProvider, ModelRouter, NvidiaProvider, OllamaProvider};
use companion_observability::{AuditLedger, MetricsCollector};
use companion_policy::{HitlApprovalGate, PolicyEvaluator, PolicyRule, SecurityRedactor};
use companion_profile::ProfileManager;
use companion_protocol::requests::{CreateTaskRequest, PromoteSkillRequest, RollbackSkillRequest, VerifyAuditRequest};
use companion_protocol::responses::{
    AuditVerificationResponse, ErrorResponse, HealthResponse, ReadyResponse, SkillOperationResponse,
    SkillSummaryResponse, TaskDetailResponse, TaskResponse,
};
use companion_protocol::stream::TaskStreamEvent;
use companion_runtime::RuntimeEngine;
use companion_skills::SkillRegistry;
use companion_storage::{create_pool, PgEventStore, PgTaskStore};
use companion_workflow::{PriorityScheduler, SwarmCoordinator, WorkerPool, WorkflowEngine};

#[derive(Clone)]
struct AppState {
    engine: Arc<RuntimeEngine>,
    workflow_engine: Arc<WorkflowEngine>,
    metrics: Arc<MetricsCollector>,
    audit_ledger: Arc<AuditLedger>,
    redactor: Arc<SecurityRedactor>,
    skill_registry: Arc<SkillRegistry>,
    hitl_gate: Arc<HitlApprovalGate>,
    policy_evaluator: Arc<tokio::sync::RwLock<PolicyEvaluator>>,
    swarm_coordinator: Arc<SwarmCoordinator>,
    profile_manager: Arc<ProfileManager>,
    start_time: Instant,
}

#[derive(Deserialize)]
struct LimitQuery {
    limit: Option<usize>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    companion_observability::init();
    info!("Starting Companion Enterprise API Gateway...");

    // Initialize Profile, Persona & Secrets Vault
    let profile_manager = Arc::new(ProfileManager::discover());
    let vault_secrets = profile_manager.secrets().known_secret_values();

    let database_url = profile_manager
        .secrets()
        .resolve_handle("postgres_url")
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .unwrap_or_else(|| "postgres://companion:companion_dev@localhost:5432/companion".into());

    let pool = create_pool(&database_url).await.expect("Failed to connect to PostgreSQL database");
    info!("Connected to PostgreSQL at {}", database_url);

    // Initialize observability & security
    let metrics = Arc::new(MetricsCollector::new());
    let audit_ledger = Arc::new(AuditLedger::new());
    let redactor = Arc::new(SecurityRedactor::new().with_secrets(vault_secrets));
    let skill_registry = Arc::new(SkillRegistry::new());

    // Initialize capabilities
    let mut cap_registry = CapabilityRegistry::new();
    register_builtins(&mut cap_registry);
    let cap_registry = Arc::new(cap_registry);

    // Initialize model router & providers
    let mut model_router = ModelRouter::new();

    let ollama_url = std::env::var("OLLAMA_URL").unwrap_or_else(|_| "http://localhost:11434".into());
    let ollama_model = std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "minimax-m3:cloud".into());
    let ollama = Arc::new(
        OllamaProvider::new(ollama_url)
            .with_models(vec![ollama_model, "gemma3:12b".into(), "qwen2.5-coder".into()]),
    );
    model_router.register(ollama);

    let default_model_setting = profile_manager
        .secrets()
        .resolve_handle("default_model")
        .or_else(|| std::env::var("DEFAULT_MODEL").ok())
        .unwrap_or_else(|| "gemini-3.7-flash".into());

    let mut default_provider = "ollama";

    if let Some(gemini_key) = profile_manager
        .secrets()
        .resolve_handle("gemini_api_key")
        .or_else(|| std::env::var("GEMINI_API_KEY").ok())
    {
        if !gemini_key.trim().is_empty() {
            let mut models = vec![
                default_model_setting.clone(),
                "gemini-3.7-flash".into(),
                "gemini-2.5-flash".into(),
                "gemini-2.5-pro".into(),
                "gemini-2.0-flash".into(),
                "gemini-1.5-flash".into(),
                "gemini-1.5-pro".into(),
            ];
            models.dedup();
            let gemini = Arc::new(GeminiProvider::new(gemini_key).with_models(models));
            model_router.register(gemini);
            if default_model_setting.starts_with("gemini") || default_model_setting.starts_with("google/") || default_model_setting == "default" {
                default_provider = "gemini";
            }
        }
    }

    if let Some(nvidia_key) = profile_manager
        .secrets()
        .resolve_handle("nvidia_api_key")
        .or_else(|| std::env::var("NVIDIA_API_KEY").ok())
    {
        if !nvidia_key.trim().is_empty() {
            let nvidia = Arc::new(NvidiaProvider::new(nvidia_key));
            model_router.register(nvidia);
            if default_model_setting.starts_with("nvidia") || default_model_setting.starts_with("meta/") || default_model_setting.starts_with("mistralai/") {
                default_provider = "nvidia";
            }
        }
    }

    model_router.set_default(default_provider);
    info!(default_provider = %default_provider, default_model = %default_model_setting, "Configured model router");
    let model_router = Arc::new(model_router);

    // Initialize stores
    let event_store = Arc::new(PgEventStore::new(pool.clone()));
    let task_store = Arc::new(PgTaskStore::new(pool.clone()));

    // Initialize Phase 10: HITL Gate, Policy Evaluator, Self-Healing Loop, Swarm
    let hitl_gate = Arc::new(HitlApprovalGate::new(std::time::Duration::from_secs(3600)));
    let policy_evaluator = Arc::new(tokio::sync::RwLock::new(PolicyEvaluator::new()));
    let self_healing_loop = Arc::new(companion_runtime::SelfHealingLoop::new());

    let worker_pool = Arc::new(WorkerPool::new(10));
    let scheduler = Arc::new(PriorityScheduler::new());
    let swarm_coordinator = Arc::new(SwarmCoordinator::new(worker_pool, scheduler));

    // Initialize MemoryOS (7 tiers) and ContextOS (8-stage compiler)
    let embedder: Arc<dyn companion_memory::EmbeddingProvider> =
        Arc::new(companion_memory::MockEmbeddingProvider::new());
    let memory_manager = Arc::new(companion_memory::MemoryManager::new(embedder));
    let context_compiler = Arc::new(companion_context::ContextCompiler::new());

    // Initialize runtime engine with MemoryOS, ContextOS, Phase 10, and Profile extensions
    let engine = Arc::new(
        RuntimeEngine::new(
            model_router,
            cap_registry,
            event_store,
            task_store,
        )
        .with_context_compiler(context_compiler)
        .with_memory_manager(memory_manager.clone())
        .with_skill_registry(skill_registry.clone())
        .with_hitl_gate(hitl_gate.clone())
        .with_self_healing_loop(self_healing_loop)
        .with_profile_manager(profile_manager.clone()),
    );

    // Initialize CAP Router, Agent Team, and Workflow Engine
    let cap_router = Arc::new(CapRouter::new());
    let team = Arc::new(AgentTeam::new(cap_router, engine.clone()));
    team.spawn_default_team().await.expect("Failed to spawn agent team");

    let workflow_engine = Arc::new(WorkflowEngine::new(team));

    let state = AppState {
        engine,
        workflow_engine,
        metrics,
        audit_ledger,
        redactor,
        skill_registry,
        hitl_gate,
        policy_evaluator,
        swarm_coordinator,
        profile_manager,
        start_time: Instant::now(),
    };

    let app = Router::new()
        // UI Dashboard
        .route("/", get(dashboard_handler))
        .route("/dashboard", get(dashboard_handler))
        // Core probes & metrics
        .route("/v1/health", get(health_handler))
        .route("/v1/ready", get(ready_handler))
        .route("/metrics", get(metrics_handler))
        // Tasks & CRP SSE streaming
        .route("/v1/tasks", post(create_task_handler))
        .route("/v1/tasks/{id}", get(get_task_handler))
        .route("/v1/tasks/{id}/events", get(get_task_events_handler))
        .route("/v1/tasks/{id}/stream", get(stream_task_events_handler))
        // Skills
        .route("/v1/skills", get(list_skills_handler))
        .route("/v1/skills/{name}/promote", post(promote_skill_handler))
        .route("/v1/skills/{name}/rollback", post(rollback_skill_handler))
        // Audit Ledger
        .route("/v1/audit/ledger", get(get_audit_ledger_handler))
        .route("/v1/audit/verify", post(verify_audit_ledger_handler))
        // Workflows
        .route("/v1/workflows", post(create_workflow_handler))
        .route("/v1/workflows/{id}/checkpoints", get(get_workflow_checkpoint_handler))
        // Phase 10: HITL Approvals, Policy Governance & Swarm Status
        .route("/v1/approvals", get(list_approvals_handler))
        .route("/v1/approvals/{id}/approve", post(approve_request_handler))
        .route("/v1/approvals/{id}/deny", post(deny_request_handler))
        .route("/v1/policies", get(list_policies_handler).post(create_policy_handler))
        .route("/v1/swarm/status", get(swarm_status_handler))
        // Identity & Secrets Metadata
        .route("/v1/profile", get(get_profile_handler))
        .route("/v1/secrets", get(list_secrets_handler))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let port = std::env::var("PORT").unwrap_or_else(|_| "8000".into());
    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("Companion Enterprise Gateway listening on http://{}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn dashboard_handler() -> impl IntoResponse {
    axum::response::Html(include_str!("dashboard.html"))
}

async fn health_handler(State(state): State<AppState>) -> impl IntoResponse {
    let health_map = state.engine.model_router().health().await;
    Json(HealthResponse {
        status: "healthy".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        providers: health_map,
    })
}

async fn ready_handler(State(state): State<AppState>) -> impl IntoResponse {
    let uptime = state.start_time.elapsed().as_secs();
    Json(ReadyResponse {
        ready: true,
        storage_connected: true,
        default_provider_ready: true,
        uptime_seconds: uptime,
    })
}

async fn metrics_handler(State(state): State<AppState>) -> impl IntoResponse {
    let body = state.metrics.export_prometheus().await;
    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/plain; version=0.0.4"),
        )
        .body(body)
        .unwrap()
}

async fn create_task_handler(
    State(state): State<AppState>,
    Json(payload): Json<CreateTaskRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    // Redact any secrets/PII in user prompt before processing
    let sanitized_input = state.redactor.redact(&payload.input);

    // Maintain consistent session tenant ID for multi-turn conversational context
    static SESSION_TENANT_ID: std::sync::LazyLock<TenantId> = std::sync::LazyLock::new(TenantId::new);
    let tenant_id = *SESSION_TENANT_ID;

    state.audit_ledger.append(
        tenant_id,
        None,
        "api:user",
        "task.submit",
        serde_json::json!({"input_length": sanitized_input.len()}),
    ).await;

    let (task_id, contract) = state
        .engine
        .compile_contract(&sanitized_input, Some(tenant_id), None, payload.workspace);

    let engine = state.engine.clone();
    let audit_ledger = state.audit_ledger.clone();
    let metrics = state.metrics.clone();
    let contract_clone = contract.clone();

    // Spawn execution in background so client receives task_id immediately and can stream/poll live events
    tokio::spawn(async move {
        let final_res = engine.run_contract(contract_clone).await;
        let is_success = matches!(&final_res, Ok(s) if matches!(s, companion_domain::TaskState::Completed));
        metrics.record_task(is_success, false);

        if let Ok(final_state) = final_res {
            audit_ledger.append(
                tenant_id,
                Some(task_id),
                "runtime:engine",
                "task.complete",
                serde_json::json!({"state": format!("{:?}", final_state)}),
            ).await;
        }
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(TaskResponse {
            task_id: *task_id.as_uuid(),
            state: companion_domain::TaskState::Created,
            created_at: chrono::Utc::now(),
        }),
    ))
}

async fn get_task_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let task_id = TaskId::from_uuid(id);
    let task_state = state.engine.task_store().get_state(task_id).await.map_err(|e| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: e.to_string(),
                code: "TASK_NOT_FOUND".into(),
            }),
        )
    })?;

    let contract = state
        .engine
        .task_store()
        .get_contract(task_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                    code: "CONTRACT_NOT_FOUND".into(),
                }),
            )
        })?;

    Ok(Json(TaskDetailResponse {
        task_id: id,
        state: task_state,
        contract,
        created_at: chrono::Utc::now(),
    }))
}

async fn get_task_events_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let task_id = TaskId::from_uuid(id);
    let events = state
        .engine
        .event_store()
        .load_events(task_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                    code: "LOAD_EVENTS_FAILED".into(),
                }),
            )
        })?;

    Ok(Json(events))
}

async fn stream_task_events_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, Json<ErrorResponse>)> {
    let task_id = TaskId::from_uuid(id);
    let raw_events = state
        .engine
        .event_store()
        .load_events(task_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                    code: "LOAD_EVENTS_FAILED".into(),
                }),
            )
        })?;

    let sse_events: Vec<Event> = raw_events
        .into_iter()
        .map(|envelope| {
            let stream_event = match envelope.event_type {
                TaskEventType::TaskCreated => {
                    let objective = envelope.payload.get("objective")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Task started")
                        .to_string();
                    TaskStreamEvent::TaskCreated {
                        task_id: id,
                        objective,
                        created_at: envelope.timestamp,
                    }
                }
                TaskEventType::ModelCallCompleted => {
                    TaskStreamEvent::TurnCompleted {
                        task_id: id,
                        turn: envelope.sequence as u32,
                        content: format!("Model response turn {} processed", envelope.sequence),
                    }
                }
                TaskEventType::ToolCallRequested => {
                    let tool = envelope.payload.get("tool_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    TaskStreamEvent::ToolCallProposed {
                        task_id: id,
                        tool,
                        arguments: envelope.payload.clone(),
                    }
                }
                TaskEventType::ToolCallCompleted => {
                    let tool = envelope.payload.get("tool_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let ms = envelope.payload.get("execution_ms")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    TaskStreamEvent::ToolCallExecuted {
                        task_id: id,
                        tool,
                        success: true,
                        output: envelope.payload.clone(),
                        content_hash: None,
                        duration_ms: ms,
                    }
                }
                TaskEventType::TaskCompleted => {
                    TaskStreamEvent::TaskCompleted {
                        task_id: id,
                        completed_at: envelope.timestamp,
                    }
                }
                TaskEventType::TaskFailed => {
                    let reason = envelope.payload.get("reason")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Execution failed")
                        .to_string();
                    TaskStreamEvent::TaskFailed {
                        task_id: id,
                        reason,
                        failed_at: envelope.timestamp,
                    }
                }
                _ => TaskStreamEvent::Heartbeat {
                    timestamp: envelope.timestamp,
                },
            };

            Event::default()
                .event(match &stream_event {
                    TaskStreamEvent::TaskCreated { .. } => "task_created",
                    TaskStreamEvent::TurnStarted { .. } => "turn_started",
                    TaskStreamEvent::ToolCallProposed { .. } => "tool_proposed",
                    TaskStreamEvent::ToolCallExecuted { .. } => "tool_executed",
                    TaskStreamEvent::TurnCompleted { .. } => "turn_completed",
                    TaskStreamEvent::TaskCompleted { .. } => "task_completed",
                    TaskStreamEvent::TaskFailed { .. } => "task_failed",
                    TaskStreamEvent::Heartbeat { .. } => "heartbeat",
                })
                .data(serde_json::to_string(&stream_event).unwrap_or_default())
        })
        .collect();

    let stream = stream::iter(sse_events).map(Ok);
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

async fn list_skills_handler(State(state): State<AppState>) -> impl IntoResponse {
    let active_skills = state.skill_registry.list_active_skills().await;
    let mut summaries = Vec::new();

    for skill in active_skills {
        let versions = state.skill_registry.list_versions(&skill.name).await;
        summaries.push(SkillSummaryResponse {
            name: skill.name.clone(),
            active_version: skill.version.to_string(),
            total_versions: versions.len(),
            description: skill.description.clone(),
            state: format!("{:?}", skill.lifecycle_state),
        });
    }

    Json(summaries)
}

async fn promote_skill_handler(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(payload): Json<PromoteSkillRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let version_num: u32 = payload.target_version.parse().map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("Invalid numeric version: {}", payload.target_version),
                code: "INVALID_VERSION".into(),
            }),
        )
    })?;

    state
        .skill_registry
        .promote_skill(&name, version_num)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: e.to_string(),
                    code: "SKILL_PROMOTION_FAILED".into(),
                }),
            )
        })?;

    state.audit_ledger.append(
        TenantId::new(),
        None,
        "api:user",
        "skill.promote",
        serde_json::json!({
            "skill_name": name,
            "version": payload.target_version,
            "reason": payload.reason
        }),
    ).await;

    Ok(Json(SkillOperationResponse {
        success: true,
        message: format!("Skill `{name}` promoted to version `{}`", payload.target_version),
        current_version: payload.target_version,
    }))
}

async fn rollback_skill_handler(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(payload): Json<RollbackSkillRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let rolled = state
        .skill_registry
        .rollback_skill(&name, &payload.reason)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: e.to_string(),
                    code: "SKILL_ROLLBACK_FAILED".into(),
                }),
            )
        })?;

    state.audit_ledger.append(
        TenantId::new(),
        None,
        "api:user",
        "skill.rollback",
        serde_json::json!({
            "skill_name": name,
            "target_version": rolled.version,
            "reason": payload.reason
        }),
    ).await;

    state.metrics.record_skill_execution(true);

    Ok(Json(SkillOperationResponse {
        success: true,
        message: format!("Skill `{name}` rolled back to stable version `{}`", rolled.version),
        current_version: rolled.version.to_string(),
    }))
}

async fn get_audit_ledger_handler(
    State(state): State<AppState>,
    Query(query): Query<LimitQuery>,
) -> impl IntoResponse {
    let limit = query.limit.unwrap_or(50);
    let entries = state.audit_ledger.get_entries(limit).await;
    Json(entries)
}

async fn verify_audit_ledger_handler(
    State(state): State<AppState>,
    Json(_payload): Json<VerifyAuditRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let count = state.audit_ledger.count().await;
    let intact = state.audit_ledger.verify_integrity().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e,
                code: "AUDIT_INTEGRITY_VIOLATION".into(),
            }),
        )
    })?;

    Ok(Json(AuditVerificationResponse {
        intact,
        total_entries: count,
        message: if intact {
            "Cryptographic audit hash chain is intact and verified.".into()
        } else {
            "Cryptographic verification failed!".into()
        },
    }))
}

async fn create_workflow_handler(
    State(state): State<AppState>,
    Json(workflow_def): Json<WorkflowDef>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let snapshot = state
        .workflow_engine
        .execute(workflow_def, None)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                    code: "WORKFLOW_EXECUTION_FAILED".into(),
                }),
            )
        })?;

    Ok(Json(snapshot))
}

async fn get_workflow_checkpoint_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let workflow_id = WorkflowId::from_uuid(id);
    let checkpoint = state
        .workflow_engine
        .get_latest_checkpoint(workflow_id)
        .await
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "No checkpoint found for workflow".into(),
                    code: "CHECKPOINT_NOT_FOUND".into(),
                }),
            )
        })?;

    Ok(Json(checkpoint))
}

// ---------------------------------------------------------------------------
// Phase 10 Handlers: HITL Approvals, Policy Governance & Swarm Status
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ApprovePayload {
    approver: Option<String>,
}

#[derive(Deserialize)]
struct DenyPayload {
    reason: String,
}

async fn list_approvals_handler(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let pending = state.hitl_gate.list_pending(None).await;
    Json(pending)
}

async fn approve_request_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<ApprovePayload>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let approval_id = ApprovalId::from_uuid(id);
    let approver = payload.approver.unwrap_or_else(|| "admin".into());
    let req = state
        .hitl_gate
        .approve(approval_id, approver)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: e.to_string(),
                    code: "APPROVAL_ERROR".into(),
                }),
            )
        })?;

    Ok(Json(req))
}

async fn deny_request_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<DenyPayload>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let approval_id = ApprovalId::from_uuid(id);
    let req = state
        .hitl_gate
        .deny(approval_id, payload.reason)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: e.to_string(),
                    code: "APPROVAL_ERROR".into(),
                }),
            )
        })?;

    Ok(Json(req))
}

async fn list_policies_handler(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let evaluator = state.policy_evaluator.read().await;
    let rules = evaluator.list_rules().to_vec();
    Json(rules)
}

async fn create_policy_handler(
    State(state): State<AppState>,
    Json(rule): Json<PolicyRule>,
) -> impl IntoResponse {
    let mut evaluator = state.policy_evaluator.write().await;
    evaluator.add_rule(rule.clone());
    (StatusCode::CREATED, Json(rule))
}

async fn swarm_status_handler(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let pool = state.swarm_coordinator.worker_pool();
    let scheduler = state.swarm_coordinator.scheduler();

    let active_workers = pool.active_count().await;
    let max_concurrency = pool.max_concurrency();
    let completed = pool.completed_count().await;
    let queue_depth = scheduler.queue_depth().await;

    Json(serde_json::json!({
        "active_workers": active_workers,
        "max_concurrency": max_concurrency,
        "completed_tasks": completed,
        "queue_depth": queue_depth,
    }))
}

async fn get_profile_handler(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let user = state.profile_manager.user_profile();
    let agent = state.profile_manager.agent_persona();
    Json(serde_json::json!({
        "user": user,
        "agent": agent,
    }))
}

async fn list_secrets_handler(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let handles = state.profile_manager.secrets().list_handles();
    Json(serde_json::json!({
        "handles": handles,
        "count": handles.len(),
    }))
}
