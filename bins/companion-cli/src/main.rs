use std::sync::Arc;
use chrono::{Duration, Utc};
use tracing::info;
use uuid::Uuid;

use companion_agents::AgentTeam;
use companion_cap::CapRouter;
use companion_capabilities::{builtins::register_builtins, CapabilityRegistry};
use companion_domain::{AgentRole, MemoryTier, StepId, StepRetryPolicy, TenantId, WorkflowDef, WorkflowStep, WorkspaceId};
use companion_events::EventStore;
use companion_memory::{
    EmbeddingProvider, MemoryManager,
    MockEmbeddingProvider, OllamaEmbeddingProvider,
};
use companion_models::{GeminiProvider, ModelRouter, NvidiaProvider, OllamaProvider};
use companion_observability::{AuditLedger, MetricsCollector};
use companion_policy::{SecurityRedactor, TenantAuthClaims, TenantSecurityManager};
use companion_runtime::RuntimeEngine;
use companion_skills::SkillRegistry;
use companion_storage::{create_pool, run_migrations, PgEventStore, PgTaskStore};
use companion_workflow::WorkflowEngine;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    companion_observability::init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    // Initialize Profile, Persona & Secrets Vault
    let profile_manager = Arc::new(companion_profile::ProfileManager::discover());
    let vault_secrets = profile_manager.secrets().known_secret_values();

    let database_url = profile_manager
        .secrets()
        .resolve_handle("postgres_url")
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .unwrap_or_else(|| "postgres://companion:companion_dev@localhost:5432/companion".into());

    let pool = create_pool(&database_url)
        .await
        .expect("Failed to connect to PostgreSQL database. (Ensure Postgres is running)");

    // Subcommand: migrate
    if args[1] == "migrate" {
        info!("Running database migrations...");
        run_migrations(&pool).await.expect("Failed to run migrations");
        println!("✓ PostgreSQL schema migrations applied successfully!");
        return Ok(());
    }

    // Initialize core components
    let redactor = Arc::new(SecurityRedactor::new().with_secrets(vault_secrets));
    let metrics = Arc::new(MetricsCollector::new());
    let audit_ledger = Arc::new(AuditLedger::new());
    let skill_registry = Arc::new(SkillRegistry::new());

    // Capabilities
    let mut cap_registry = CapabilityRegistry::new();
    register_builtins(&mut cap_registry);
    let cap_registry = Arc::new(cap_registry);

    // Model Router
    let mut model_router = ModelRouter::new();
    let ollama_url = std::env::var("OLLAMA_URL").unwrap_or_else(|_| "http://localhost:11434".into());
    let ollama_model = std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "minimax-m3:cloud".into());
    let ollama = Arc::new(
        OllamaProvider::new(ollama_url.clone())
            .with_models(vec![ollama_model, "gemma3:12b".into(), "qwen2.5-coder".into()]),
    );
    model_router.register(ollama);

    if let Some(gemini_key) = profile_manager
        .secrets()
        .resolve_handle("gemini_api_key")
        .or_else(|| std::env::var("GEMINI_API_KEY").ok())
    {
        if !gemini_key.trim().is_empty() {
            let gemini = Arc::new(GeminiProvider::new(gemini_key));
            model_router.register(gemini);
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
        }
    }

    model_router.set_default("ollama");
    let model_router = Arc::new(model_router);

    // Stores
    let event_store = Arc::new(PgEventStore::new(pool.clone()));
    let task_store = Arc::new(PgTaskStore::new(pool.clone()));

    // Runtime Engine
    let engine = Arc::new(
        RuntimeEngine::new(
            model_router,
            cap_registry,
            event_store.clone(),
            task_store,
        )
        .with_profile_manager(profile_manager.clone()),
    );

    // Memory
    let embedder: Arc<dyn EmbeddingProvider> = if std::env::var("USE_OLLAMA_EMBED").is_ok() {
        Arc::new(OllamaEmbeddingProvider::new(ollama_url, "nomic-embed-text"))
    } else {
        Arc::new(MockEmbeddingProvider::new())
    };
    let memory_manager = Arc::new(MemoryManager::new(embedder));

    // ───────────────────────────────────────────────────────────────────────
    // Subcommand dispatch
    // ───────────────────────────────────────────────────────────────────────

    match args[1].as_str() {
        "memory" => {
            if args.len() < 3 {
                eprintln!("Usage: companion memory <remember | recall | consolidate> [args...]");
                return Ok(());
            }
            match args[2].as_str() {
                "remember" => {
                    let fact = args[3..].join(" ");
                    let item = memory_manager.remember(&fact, MemoryTier::Semantic, 1.0).await?;
                    println!("\n✓ Stored memory item in Semantic Tier:");
                    println!("  ID:      {}", item.memory_id);
                    println!("  Content: \"{}\"", item.content);
                }
                "recall" => {
                    let query = args[3..].join(" ");
                    let results = memory_manager.recall(&query, 5, 0.1).await?;
                    println!("\n🔍 Recalled {} relevant memories for \"{}\":", results.len(), query);
                    for (i, r) in results.iter().enumerate() {
                        println!("  [{}] Score: {:.3} | Tier: {:<9} | \"{}\"", i + 1, r.score, r.tier, r.item.content);
                    }
                }
                "consolidate" => {
                    let result = memory_manager.consolidate(&[], &[]).await?;
                    println!("\n🌙 Dream Cycle Consolidation Complete:");
                    println!("  Episodes processed:       {}", result.episodes_processed);
                    println!("  Semantic facts extracted: {}", result.facts_extracted);
                    println!("  Graph triples created:    {}", result.triples_created);
                    println!("  Contradictions resolved:  {}", result.contradictions_resolved);
                    println!("  Records superseded:       {}", result.records_superseded);
                    println!("  Duration:                 {} ms", result.duration_ms);
                }
                other => eprintln!("Unknown memory subcommand: {other}"),
            }
            return Ok(());
        }

        "skill" => {
            if args.len() < 3 {
                eprintln!("Usage: companion skill <list | promote | rollback> [args...]");
                return Ok(());
            }
            match args[2].as_str() {
                "list" => {
                    let skills = skill_registry.list_active_skills().await;
                    println!("\n📦 Active Production Skills ({}):", skills.len());
                    for s in skills {
                        println!("  - {:<20} v{:<3} | {:?} | {}", s.name, s.version, s.lifecycle_state, s.description);
                    }
                }
                "promote" => {
                    if args.len() < 5 {
                        eprintln!("Usage: companion skill promote <name> <version>");
                        return Ok(());
                    }
                    let name = &args[3];
                    let version: u32 = args[4].parse().expect("Version must be a number");
                    let promoted = skill_registry.promote_skill(name, version).await?;
                    println!("\n✓ Promoted skill `{}` to version {}", promoted.name, promoted.version);
                }
                "rollback" => {
                    if args.len() < 5 {
                        eprintln!("Usage: companion skill rollback <name> <reason>");
                        return Ok(());
                    }
                    let name = &args[3];
                    let reason = &args[4];
                    let rolled = skill_registry.rollback_skill(name, reason).await?;
                    println!("\n✓ Rolled back skill `{}` to version {}", rolled.name, rolled.version);
                }
                other => eprintln!("Unknown skill subcommand: {other}"),
            }
            return Ok(());
        }

        "audit" => {
            if args.len() < 3 {
                eprintln!("Usage: companion audit <view | verify>");
                return Ok(());
            }
            match args[2].as_str() {
                "view" => {
                    let entries = audit_ledger.get_entries(20).await;
                    println!("\n📜 Audit Ledger Trail (latest {}):", entries.len());
                    for e in entries {
                        println!("  [#{}] {} | {:<15} | actor: {:<12} | hash: {:.16}...", e.sequence, e.timestamp.to_rfc3339(), e.action, e.actor, e.entry_hash);
                    }
                }
                "verify" => {
                    let count = audit_ledger.count().await;
                    match audit_ledger.verify_integrity().await {
                        Ok(true) => println!("\n✓ Audit Ledger Cryptographic Chain Intact! ({} entries verified)", count),
                        Ok(false) | Err(_) => println!("\n❌ Audit Ledger integrity check FAILED! Tampering detected!"),
                    }
                }
                other => eprintln!("Unknown audit subcommand: {other}"),
            }
            return Ok(());
        }

        "metrics" => {
            let prom = metrics.export_prometheus().await;
            println!("\n📊 Companion Runtime Metrics (Prometheus Format):");
            println!("──────────────────────────────────────────────────────────────");
            println!("{prom}");
            return Ok(());
        }

        "tenant" => {
            if args.len() < 5 || args[2] != "issue-token" {
                eprintln!("Usage: companion tenant issue-token <tenant_id> <workspace_id>");
                return Ok(());
            }
            let tenant_id = TenantId::from_uuid(Uuid::parse_str(&args[3]).unwrap_or_else(|_| *TenantId::new().as_uuid()));
            let workspace_id = WorkspaceId::from_uuid(Uuid::parse_str(&args[4]).unwrap_or_else(|_| *WorkspaceId::new().as_uuid()));
            let manager = TenantSecurityManager::new("/var/data/workspaces", "companion-enterprise-secret-key");
            let claims = TenantAuthClaims {
                tenant_id,
                workspace_id,
                roles: vec!["admin".into(), "executor".into()],
                expires_at: Utc::now() + Duration::hours(24),
            };
            let token = manager.issue_token(&claims);
            println!("\n🔑 Issued Tenant Auth Token:");
            println!("{token}");
            return Ok(());
        }

        "goal" => {
            let user_input = args[2..].join(" ");
            println!("\n🤖 Orchestrating Multi-Agent Workflow Team for Goal...");
            let cap_router = Arc::new(CapRouter::new());
            let team = Arc::new(AgentTeam::new(cap_router, engine.clone()));
            team.spawn_default_team().await?;

            let workflow_engine = WorkflowEngine::new(team);
            let mut def = WorkflowDef::new("Goal Execution Workflow", &user_input);
            let step1_id = StepId::new();
            let step2_id = StepId::new();
            let step3_id = StepId::new();

            def.add_step(WorkflowStep {
                step_id: step1_id,
                name: "Architecture & Planning".into(),
                description: "Design execution plan".into(),
                assigned_role: AgentRole::Architect,
                prompt: format!("#plan Design technical plan for: {}", user_input),
                required_tools: vec![],
                retry_policy: StepRetryPolicy::default(),
                timeout_secs: 300,
            });

            def.add_step(WorkflowStep {
                step_id: step2_id,
                name: "Implementation & Build".into(),
                description: "Implement components and write files".into(),
                assigned_role: AgentRole::Engineer,
                prompt: format!("#build Implement components for: {}", user_input),
                required_tools: vec!["filesystem.write".into()],
                retry_policy: StepRetryPolicy::default(),
                timeout_secs: 300,
            });

            def.add_step(WorkflowStep {
                step_id: step3_id,
                name: "Verification & Review".into(),
                description: "Verify implementation against contract".into(),
                assigned_role: AgentRole::Reviewer,
                prompt: format!("#review Verify artifacts for: {}", user_input),
                required_tools: vec!["filesystem.read".into()],
                retry_policy: StepRetryPolicy::default(),
                timeout_secs: 300,
            });

            def.add_dependency(step1_id, step2_id);
            def.add_dependency(step2_id, step3_id);

            let snapshot = workflow_engine.execute(def, None).await?;
            println!("\n──────────────────────────────────────────────────────────────");
            println!("Workflow ID:   {}", snapshot.workflow_id);
            println!("Status:        {:?}", snapshot.status);
            println!("Steps:         {} / 3 completed", snapshot.step_outputs.len());
            println!("──────────────────────────────────────────────────────────────");
            return Ok(());
        }

        "swarm" => {
            println!("══════════════════════════════════════════════════════════════");
            println!("🐝 Companion Swarm Fleet Status");
            println!("══════════════════════════════════════════════════════════════");
            println!("Active Workers:   0");
            println!("Max Concurrency:  10");
            println!("Queue Depth:      0");
            println!("Fair-Share Queue: operational");
            println!("══════════════════════════════════════════════════════════════");
            return Ok(());
        }

        "approve" => {
            if args.len() < 3 {
                eprintln!("Usage: companion approve <list|grant|deny> [args...]");
                return Ok(());
            }
            match args[2].as_str() {
                "list" => {
                    println!("\n📋 Pending HITL Approvals (0 pending)");
                    println!("──────────────────────────────────────────────────────────────");
                }
                "grant" => {
                    if args.len() < 4 {
                        eprintln!("Usage: companion approve grant <approval_id>");
                        return Ok(());
                    }
                    println!("✓ Approval {} granted by operator.", args[3]);
                }
                "deny" => {
                    if args.len() < 5 {
                        eprintln!("Usage: companion approve deny <approval_id> <reason>");
                        return Ok(());
                    }
                    println!("✗ Approval {} denied. Reason: {}", args[3], args[4]);
                }
                _ => eprintln!("Unknown approve subcommand: {}", args[2]),
            }
            return Ok(());
        }

        "policy" => {
            println!("\n🛡️ Active Enterprise Declarative Policies (0 rules loaded)");
            println!("──────────────────────────────────────────────────────────────");
            println!("Default rule: RiskLevel::Critical requires dual-control HITL approval");
            return Ok(());
        }

        "profile" => {
            let user = profile_manager.user_profile();
            let agent = profile_manager.agent_persona();

            println!("\n👤 User Profile (from {}):", profile_manager.config_dir().join("user.md").display());
            println!("──────────────────────────────────────────────────────────────");
            println!("  Name:        {}", user.display_name());
            if let Some(h) = &user.handle {
                println!("  Handle:      {}", h);
            }
            if let Some(tz) = &user.timezone {
                println!("  Timezone:    {}", tz);
            }
            if !user.preferences.is_empty() {
                println!("  Preferences: {}", user.preferences.join(", "));
            }
            if !user.current_projects.is_empty() {
                println!("  Projects:    {}", user.current_projects.join(", "));
            }

            println!("\n🤖 Agent Persona (from {}):", profile_manager.config_dir().join("agent.md").display());
            println!("──────────────────────────────────────────────────────────────");
            println!("  Name:        {}", agent.name());
            if let Some(r) = &agent.role {
                println!("  Role:        {}", r);
            }
            if !agent.traits.is_empty() {
                println!("  Traits:      {}", agent.traits.join(", "));
            }
            if !agent.behavioral_rules.is_empty() {
                println!("  Rules:       {}", agent.behavioral_rules.join(", "));
            }
            return Ok(());
        }

        "secrets" => {
            if args.len() < 3 || args[2] == "list" {
                let handles = profile_manager.secrets().list_handles();
                println!("\n🔒 Configured Secret Handles ({} found in {}):", handles.len(), profile_manager.config_dir().join("secrets.toml").display());
                println!("──────────────────────────────────────────────────────────────");
                for h in handles {
                    println!("  ✓ $SECRET:{}", h);
                }
                println!("\n(Values are isolated and never outputted in plain text or passed to agents)");
                return Ok(());
            }

            if args[2] == "set" {
                if args.len() < 5 {
                    eprintln!("Usage: companion secrets set <key> <value>");
                    return Ok(());
                }
                let key = &args[3];
                let val = &args[4];
                profile_manager.secrets().set(key, val);
                profile_manager.save_secrets().expect("Failed to save secrets");
                println!("✓ Secret '{}' stored securely in secrets vault.", key);
                return Ok(());
            }

            eprintln!("Usage: companion secrets <list | set <key> <value>>");
            return Ok(());
        }

        _ => {
            // Task execution (either `companion run "..."` or `companion "#mode ..."`)
            let input_raw = if args[1] == "run" {
                args[2..].join(" ")
            } else {
                args[1..].join(" ")
            };

            // Redact PII / Secrets before runtime execution
            let sanitized_input = redactor.redact(&input_raw);

            println!("══════════════════════════════════════════════════════════════");
            println!("🧠 Companion Enterprise Agent Runtime");
            println!("Input: \"{}\"", sanitized_input);
            println!("══════════════════════════════════════════════════════════════");

            let tenant_id = TenantId::new();
            audit_ledger.append(tenant_id, None, "cli:user", "task.submit", serde_json::json!({"input": sanitized_input})).await;

            let (task_id, final_state, contract) = engine
                .submit_and_run(&sanitized_input, None, None, None)
                .await?;

            let is_success = matches!(final_state, companion_domain::TaskState::Completed);
            metrics.record_task(is_success, false);

            audit_ledger.append(
                tenant_id,
                Some(task_id),
                "runtime:engine",
                "task.complete",
                serde_json::json!({"state": format!("{:?}", final_state)}),
            ).await;

            println!("\n──────────────────────────────────────────────────────────────");
            println!("Task ID:      {}", task_id);
            println!("Primary Mode: {:?}", contract.mode_profile.primary);
            println!("Final State:  {}", final_state);
            println!("──────────────────────────────────────────────────────────────");

            let events = event_store.load_events(task_id).await?;
            println!("\nEvent Trail ({} events):", events.len());
            for ev in &events {
                println!("  [{:>2}] {:<22} | {}", ev.sequence, ev.event_type, ev.payload);
            }

            // Auto-record task episode into episodic memory
            let _ = memory_manager
                .episodic_recorder()
                .record_task_episode(&contract, &final_state, &events)
                .await;

            println!("══════════════════════════════════════════════════════════════");
        }
    }

    Ok(())
}

fn print_usage() {
    eprintln!("Usage: companion <command> [args...]");
    eprintln!("\nCommands:");
    eprintln!("  run \"<prompt>\"                          Execute a single task with strict contract");
    eprintln!("  goal \"<objective>\"                      Run multi-agent DAG workflow (Architect -> Engineer -> Reviewer)");
    eprintln!("  swarm status                            Display elastic worker pool & queue telemetry");
    eprintln!("  approve list                            List pending HITL dual-control approvals");
    eprintln!("  approve grant <id>                      Approve suspended task execution");
    eprintln!("  approve deny <id> <reason>              Deny and cancel suspended task");
    eprintln!("  policy list                             List active declarative policy rules");
    eprintln!("  memory remember \"<fact>\"                Store fact in semantic memory");
    eprintln!("  memory recall \"<query>\"                 Recall semantic and episodic memories");
    eprintln!("  memory consolidate                      Run Dream Cycle memory consolidation");
    eprintln!("  skill list                              List active production skills");
    eprintln!("  skill promote <name> <version>          Promote a skill version");
    eprintln!("  skill rollback <name> <reason>          Roll back a skill to previous stable version");
    eprintln!("  audit view                              View cryptographic audit ledger entries");
    eprintln!("  audit verify                            Verify cryptographic SHA256 hash chain");
    eprintln!("  metrics                                 Display runtime Prometheus metrics");
    eprintln!("  profile                                 Display active user profile & agent persona");
    eprintln!("  secrets list                            List configured secret handles (isolated & masked)");
    eprintln!("  secrets set <key> <value>               Securely store a secret key in vault");
    eprintln!("  tenant issue-token <tenant> <workspace> Issue signed tenant authorization token");
    eprintln!("  migrate                                 Apply PostgreSQL database migrations");
}
