use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};

use companion_capabilities::CapabilityRegistry;
use companion_domain::{
    Evidence, EvidenceAuthority, Message, ModelError, ModelRequest, Observation,
    RuntimeError, TaskContract, TaskState, ToolDefinition, VerificationVerdict,
};
use companion_events::{EventStore, TaskEvent, TaskEventType};
use companion_models::ModelRouter;
use companion_policy::{AuthorizationDecision, AuthorizationGate, ToolIntentMonitor, TurnVerdict};
use companion_storage::TaskStore;

use crate::verifier::Verifier;

pub struct ExecutionLoop {
    contract: TaskContract,
    state: TaskState,
    sequence: i64,
    messages: Vec<Message>,
    evidence: Vec<Evidence>,
    turns_taken: u32,
    tool_calls_count: u32,

    model_router: Arc<ModelRouter>,
    capability_registry: Arc<CapabilityRegistry>,
    event_store: Arc<dyn EventStore>,
    task_store: Arc<dyn TaskStore>,
    policy_monitor: Arc<ToolIntentMonitor>,
    authz_gate: Arc<AuthorizationGate>,
    verifier: Arc<Verifier>,
    context_compiler: Option<Arc<companion_context::ContextCompiler>>,
    memory_manager: Option<Arc<companion_memory::MemoryManager>>,
    skill_registry: Option<Arc<companion_skills::SkillRegistry>>,
    policy_evaluator: Option<Arc<companion_policy::PolicyEvaluator>>,
    hitl_gate: Option<Arc<companion_policy::HitlApprovalGate>>,
    self_healing_loop: Option<Arc<crate::self_healing::SelfHealingLoop>>,
    profile_manager: Option<Arc<companion_profile::ProfileManager>>,
}

impl ExecutionLoop {
    pub fn new(
        contract: TaskContract,
        model_router: Arc<ModelRouter>,
        capability_registry: Arc<CapabilityRegistry>,
        event_store: Arc<dyn EventStore>,
        task_store: Arc<dyn TaskStore>,
        policy_monitor: Arc<ToolIntentMonitor>,
        authz_gate: Arc<AuthorizationGate>,
        verifier: Arc<Verifier>,
    ) -> Self {
        Self {
            contract,
            state: TaskState::Created,
            sequence: 0,
            messages: Vec::new(),
            evidence: Vec::new(),
            turns_taken: 0,
            tool_calls_count: 0,
            model_router,
            capability_registry,
            event_store,
            task_store,
            policy_monitor,
            authz_gate,
            verifier,
            context_compiler: None,
            memory_manager: None,
            skill_registry: None,
            policy_evaluator: None,
            hitl_gate: None,
            self_healing_loop: None,
            profile_manager: None,
        }
    }

    pub fn with_context_compiler(mut self, compiler: Arc<companion_context::ContextCompiler>) -> Self {
        self.context_compiler = Some(compiler);
        self
    }

    pub fn with_memory_manager(mut self, memory_manager: Arc<companion_memory::MemoryManager>) -> Self {
        self.memory_manager = Some(memory_manager);
        self
    }

    pub fn with_skill_registry(mut self, skill_registry: Arc<companion_skills::SkillRegistry>) -> Self {
        self.skill_registry = Some(skill_registry);
        self
    }

    pub fn with_policy_evaluator(mut self, evaluator: Arc<companion_policy::PolicyEvaluator>) -> Self {
        self.policy_evaluator = Some(evaluator);
        self
    }

    pub fn with_hitl_gate(mut self, hitl_gate: Arc<companion_policy::HitlApprovalGate>) -> Self {
        self.hitl_gate = Some(hitl_gate);
        self
    }

    pub fn with_self_healing_loop(mut self, self_healing: Arc<crate::self_healing::SelfHealingLoop>) -> Self {
        self.self_healing_loop = Some(self_healing);
        self
    }

    pub fn with_profile_manager(mut self, profile_manager: Arc<companion_profile::ProfileManager>) -> Self {
        self.profile_manager = Some(profile_manager);
        self
    }

    async fn record_event(&mut self, event_type: TaskEventType, payload: serde_json::Value) -> Result<(), RuntimeError> {
        self.sequence += 1;
        let event = TaskEvent::new(
            self.contract.task_id,
            self.contract.correlation_id,
            self.sequence,
            event_type,
            payload,
        );

        self.event_store
            .append(event)
            .await
            .map_err(|e| RuntimeError::StorageError(e.to_string()))?;

        Ok(())
    }

    async fn transition(&mut self, next: TaskState) -> Result<(), RuntimeError> {
        let from = self.state.clone();
        let validated_next = self.state.clone().transition(next).map_err(|e| {
            RuntimeError::InvalidTransition {
                from: e.from.to_string(),
                to: e.to_string(),
            }
        })?;

        self.state = validated_next.clone();

        self.record_event(
            TaskEventType::StateTransition,
            serde_json::json!({
                "from": from.to_string(),
                "to": validated_next.to_string(),
            }),
        )
        .await?;

        self.task_store
            .update_state(self.contract.task_id, &self.state)
            .await
            .map_err(|e| RuntimeError::StorageError(e.to_string()))?;

        info!(task_id = %self.contract.task_id, from = %from, to = %self.state, "state transition");
        Ok(())
    }

    /// Execute the full task lifecycle to a terminal state.
    pub async fn run(&mut self) -> Result<TaskState, RuntimeError> {
        // Initial setup
        self.task_store
            .save(self.contract.task_id, &self.state, &self.contract)
            .await
            .map_err(|e| RuntimeError::StorageError(e.to_string()))?;

        self.record_event(
            TaskEventType::TaskCreated,
            serde_json::json!({
                "objective": self.contract.objective,
                "user_input": self.contract.user_input,
            }),
        )
        .await?;

        // 1. Created -> Planning
        self.transition(TaskState::Planning).await?;

        // Setup system prompt and user message via ContextOS or default
        let (user_profile_block, agent_persona_block) = if let Some(pm) = &self.profile_manager {
            (
                Some(pm.user_profile().as_context_block()).filter(|s| !s.is_empty()),
                Some(pm.agent_persona().as_system_prompt_prefix()).filter(|s| !s.is_empty()),
            )
        } else {
            (None, None)
        };

        // Live Workspace Blueprint
        let workspace_root = self.contract.constraints.iter().find_map(|c| match c {
            companion_domain::Constraint::WorkspaceRoot { path } => Some(path.clone()),
            _ => None,
        });

        let blueprint_block = if let Some(ref root_path) = workspace_root {
            companion_context::WorkspaceBlueprint::discover(root_path).as_context_block()
        } else {
            companion_context::WorkspaceBlueprint::embedded_default().as_context_block()
        };

        if let (Some(compiler), Some(mem)) = (&self.context_compiler, &self.memory_manager) {
            let recalled = mem.recall(&self.contract.objective, 5, 0.1).await.unwrap_or_default();
            let selected_skills = if let Some(reg) = &self.skill_registry {
                reg.match_skills_for_task(&self.contract.objective, &self.contract.allowed_tools).await
            } else {
                Vec::new()
            };
            let session_id = companion_domain::SessionId::from(*self.contract.tenant_id.as_uuid());
            let session_turns = mem.session_store().get_recent_turns(&session_id, 12).await;
            let sources = companion_domain::ContextSources {
                identity_policy: Some(format!(
                    "You are Companion, a precise, deterministic AI assistant operating in mode '{:?}'. Follow constraints rigorously. Call tools to execute actions.",
                    self.contract.mode_profile.primary
                )),
                user_profile_block: user_profile_block.clone(),
                agent_persona_block: agent_persona_block.clone(),
                task_contract: Some(self.contract.clone()),
                workspace_blueprint: Some(blueprint_block.clone()),
                selected_skills,
                recalled_memories: recalled,
                session_turns,
                user_input: Some(self.contract.objective.clone()),
                ..Default::default()
            };
            let budget = companion_domain::ContextBudget::for_total_tokens(4096);
            if let Ok(compiled) = compiler.compile(&sources, &budget, None).await {
                self.messages = compiled.messages;
            } else {
                let mut sys_blocks = Vec::new();
                if let Some(p) = &agent_persona_block {
                    sys_blocks.push(p.clone());
                }
                sys_blocks.push(format!(
                    "You are Companion, a precise, deterministic AI assistant operating in mode '{:?}'.\n\
                     Objective: {}\n\
                     Allowed tools: {:?}\n\
                     Follow constraints rigorously. Call tools to execute actions.",
                    self.contract.mode_profile.primary,
                    self.contract.objective,
                    self.contract.allowed_tools
                ));
                sys_blocks.push(blueprint_block.clone());
                if let Some(u) = &user_profile_block {
                    sys_blocks.push(u.clone());
                }

                self.messages.push(Message::system(sys_blocks.join("\n\n")));
                self.messages.push(Message::user(self.contract.objective.clone()));
            }
        } else {
            let mut sys_blocks = Vec::new();
            if let Some(p) = &agent_persona_block {
                sys_blocks.push(p.clone());
            }
            sys_blocks.push(format!(
                "You are Companion, a precise, deterministic AI assistant operating in mode '{:?}'.\n\
                 Objective: {}\n\
                 Allowed tools: {:?}\n\
                 Follow constraints rigorously. Call tools to execute actions.",
                self.contract.mode_profile.primary,
                self.contract.objective,
                self.contract.allowed_tools
            ));
            sys_blocks.push(blueprint_block.clone());
            if let Some(u) = &user_profile_block {
                sys_blocks.push(u.clone());
            }

            self.messages.push(Message::system(sys_blocks.join("\n\n")));
            self.messages.push(Message::user(self.contract.objective.clone()));
        }

        self.record_event(
            TaskEventType::TaskContractCompiled,
            serde_json::to_value(&self.contract).unwrap_or_default(),
        )
        .await?;

        // 2. Planning -> Ready
        self.transition(TaskState::Ready).await?;

        // 3. Ready -> Executing
        self.transition(TaskState::Executing).await?;

        // Check policy evaluator and HITL gate at start of execution
        let maybe_hitl_gate = self.hitl_gate.clone();
        let maybe_policy_evaluator = self.policy_evaluator.clone();

        if let Some(evaluator) = maybe_policy_evaluator {
            let decision = evaluator.evaluate(&self.contract, "task_execution");
            match decision.effect {
                companion_policy::PolicyEffect::Deny { reason } => {
                    let fail_reason = format!("Policy Denied: {reason}");
                    self.fail(&fail_reason).await?;
                    return Ok(self.state.clone());
                }
                companion_policy::PolicyEffect::RequireApproval { .. } => {
                    if let Some(gate) = maybe_hitl_gate {
                        let req = gate
                            .request_approval(
                                self.contract.task_id,
                                self.contract.tenant_id,
                                self.contract.risk_level,
                                self.contract.objective.clone(),
                                self.contract.allowed_tools.clone(),
                            )
                            .await;

                        self.record_event(
                            TaskEventType::ApprovalRequested,
                            serde_json::json!({
                                "approval_id": req.approval_id.to_string(),
                                "risk_level": format!("{:?}", req.risk_level),
                            }),
                        )
                        .await?;

                        self.transition(TaskState::Suspended {
                            reason: "HITL dual-control approval required by policy".into(),
                            approval_id: req.approval_id.to_string(),
                        })
                        .await?;

                        if let Some(current) = gate.get(req.approval_id).await {
                            match current.status {
                                companion_policy::ApprovalStatus::Approved { .. } => {
                                    self.record_event(
                                        TaskEventType::ApprovalResolved,
                                        serde_json::json!({ "approval_id": req.approval_id.to_string(), "status": "approved" }),
                                    )
                                    .await?;
                                    self.transition(TaskState::Executing).await?;
                                }
                                companion_policy::ApprovalStatus::Denied { reason, .. } => {
                                    self.record_event(
                                        TaskEventType::ApprovalResolved,
                                        serde_json::json!({ "approval_id": req.approval_id.to_string(), "status": "denied", "reason": reason }),
                                    )
                                    .await?;
                                    self.transition(TaskState::Cancelled).await?;
                                    return Ok(self.state.clone());
                                }
                                _ => {
                                    return Ok(self.state.clone());
                                }
                            }
                        } else {
                            return Ok(self.state.clone());
                        }
                    }
                }
                _ => {}
            }
        } else if self.contract.risk_level == companion_domain::RiskLevel::Critical {
            if let Some(gate) = maybe_hitl_gate {
                let req = gate
                    .request_approval(
                        self.contract.task_id,
                        self.contract.tenant_id,
                        self.contract.risk_level,
                        self.contract.objective.clone(),
                        self.contract.allowed_tools.clone(),
                    )
                    .await;

                self.record_event(
                    TaskEventType::ApprovalRequested,
                    serde_json::json!({
                        "approval_id": req.approval_id.to_string(),
                        "risk_level": format!("{:?}", req.risk_level),
                    }),
                )
                .await?;

                self.transition(TaskState::Suspended {
                    reason: "Critical risk task requires HITL approval".into(),
                    approval_id: req.approval_id.to_string(),
                })
                .await?;

                if let Some(current) = gate.get(req.approval_id).await {
                    match current.status {
                        companion_policy::ApprovalStatus::Approved { .. } => {
                            self.record_event(
                                TaskEventType::ApprovalResolved,
                                serde_json::json!({ "approval_id": req.approval_id.to_string(), "status": "approved" }),
                            )
                            .await?;
                            self.transition(TaskState::Executing).await?;
                        }
                        companion_policy::ApprovalStatus::Denied { reason, .. } => {
                            self.record_event(
                                TaskEventType::ApprovalResolved,
                                serde_json::json!({ "approval_id": req.approval_id.to_string(), "status": "denied", "reason": reason }),
                            )
                            .await?;
                            self.transition(TaskState::Cancelled).await?;
                            return Ok(self.state.clone());
                        }
                        _ => {
                            return Ok(self.state.clone());
                        }
                    }
                } else {
                    return Ok(self.state.clone());
                }
            }
        }

        let mut repair_attempts = 0;
        let mut self_healing_attempts = 0;
        const MAX_REPAIR_ATTEMPTS: u32 = 3;

        // Main execution loop
        while !self.state.is_terminal() {
            // Check budget
            if self.turns_taken >= self.contract.budget.max_turns {
                let reason = format!("Max turns exceeded: {}", self.contract.budget.max_turns);
                if !self.attempt_self_healing(&reason, &mut self_healing_attempts).await? {
                    self.fail(&reason).await?;
                    break;
                }
                continue;
            }
            if self.tool_calls_count >= self.contract.budget.max_tool_calls {
                let reason = format!("Max tool calls exceeded: {}", self.contract.budget.max_tool_calls);
                if !self.attempt_self_healing(&reason, &mut self_healing_attempts).await? {
                    self.fail(&reason).await?;
                    break;
                }
                continue;
            }

            self.turns_taken += 1;

            // Prepare tool definitions for model
            let allowed_defs: Vec<ToolDefinition> = self
                .capability_registry
                .definitions_for(&self.contract.allowed_tools)
                .into_iter()
                .map(ToolDefinition::from)
                .collect();

            let model_request = ModelRequest {
                model: "default".into(), // Router resolves default or override
                messages: self.messages.clone(),
                tools: allowed_defs,
                tool_choice: companion_domain::ToolChoice::Auto,
                temperature: 0.2,
                max_tokens: Some(4096),
                stream: false,
            };

            self.record_event(
                TaskEventType::ModelCallStarted,
                serde_json::json!({
                    "turn": self.turns_taken,
                }),
            )
            .await?;

            // Generate model response with retries
            let model_response = match self.generate_with_retry(model_request).await {
                Ok(resp) => {
                    self.record_event(
                        TaskEventType::ModelCallCompleted,
                        serde_json::json!({
                            "content": resp.content,
                            "tool_calls_count": resp.tool_calls.len(),
                            "usage": resp.usage,
                        }),
                    )
                    .await?;
                    resp
                }
                Err(e) => {
                    let reason = format!("Model generation failed: {e}");
                    if !self.attempt_self_healing(&reason, &mut self_healing_attempts).await? {
                        self.fail(&reason).await?;
                        break;
                    }
                    continue;
                }
            };

            // Policy check turn
            let verdict = self.policy_monitor.evaluate_turn(&self.contract, &model_response, &self.evidence);
            match verdict {
                TurnVerdict::Rejected { reason, missing_capabilities } => {
                    warn!(reason = %reason, "turn rejected by policy monitor");
                    self.record_event(
                        TaskEventType::TurnRejected,
                        serde_json::json!({
                            "reason": reason,
                            "missing_capabilities": missing_capabilities,
                        }),
                    )
                    .await?;

                    // Inject correction message
                    self.messages.push(Message::user(format!(
                        "POLICY REJECTION: {reason}. You must call one of: {missing_capabilities:?} to fulfill the contract."
                    )));
                    continue;
                }
                TurnVerdict::Accepted => {}
            }

            // Handle tool calls if proposed
            if model_response.has_tool_calls() {
                self.messages.push(Message::assistant_with_tool_calls(
                    model_response.tool_calls.clone(),
                ));

                for tool_call in &model_response.tool_calls {
                    self.tool_calls_count += 1;

                    // Evaluate declarative policy rules for this tool call
                    if let Some(evaluator) = &self.policy_evaluator {
                        let decision = evaluator.evaluate(&self.contract, &tool_call.name);
                        if let companion_policy::PolicyEffect::Deny { reason } = decision.effect {
                            self.record_event(
                                TaskEventType::AuthorizationDecision,
                                serde_json::json!({
                                    "tool": tool_call.name,
                                    "authorized": false,
                                    "reason": format!("Policy Denied: {reason}"),
                                }),
                            )
                            .await?;
                            self.messages.push(Message::tool(
                                tool_call.id.clone(),
                                serde_json::json!({"error": format!("Policy violation: {reason}")}).to_string(),
                            ));
                            continue;
                        }
                    }

                    // Authorize tool call via standard gate
                    let auth = self.authz_gate.authorize(&self.contract, tool_call);
                    match auth {
                        AuthorizationDecision::Denied { tool, reason } => {
                            self.record_event(
                                TaskEventType::AuthorizationDecision,
                                serde_json::json!({
                                    "tool": tool,
                                    "authorized": false,
                                    "reason": reason,
                                }),
                            )
                            .await?;

                            self.messages.push(Message::tool(
                                tool_call.id.clone(),
                                serde_json::json!({"error": reason}).to_string(),
                            ));
                            continue;
                        }
                        AuthorizationDecision::Allowed => {
                            self.record_event(
                                TaskEventType::AuthorizationDecision,
                                serde_json::json!({
                                    "tool": tool_call.name,
                                    "authorized": true,
                                }),
                            )
                            .await?;
                        }
                    }

                    // Transition to WaitingTool
                    self.transition(TaskState::WaitingTool {
                        tool_call_id: tool_call.id.clone(),
                    })
                    .await?;

                    self.record_event(
                        TaskEventType::ToolCallStarted,
                        serde_json::json!({
                            "id": tool_call.id,
                            "name": tool_call.name,
                            "arguments": tool_call.arguments,
                        }),
                    )
                    .await?;

                    // Execute tool
                    let tool_result = self.capability_registry.execute(tool_call).await;

                    // Transition back to Executing
                    self.transition(TaskState::Executing).await?;

                    match tool_result {
                        Ok(res) => {
                            self.record_event(
                                TaskEventType::ToolCallCompleted,
                                serde_json::json!({
                                    "id": tool_call.id,
                                    "success": res.success,
                                    "output": res.output,
                                    "execution_ms": res.execution_ms,
                                }),
                            )
                            .await?;

                            // Add evidence
                            let obs = Observation {
                                kind: "tool_execution".into(),
                                value: serde_json::json!({
                                    "name": tool_call.name,
                                    "success": res.success,
                                    "output": res.output,
                                }),
                                authority: EvidenceAuthority::DeterministicRuntime,
                                timestamp: chrono::Utc::now(),
                            };
                            let ev = Evidence::from_observation(
                                self.contract.task_id,
                                format!("Executed tool {}", tool_call.name),
                                obs,
                            );
                            self.evidence.push(ev);

                            self.messages.push(Message::tool(
                                tool_call.id.clone(),
                                serde_json::to_string(&res.output).unwrap_or_default(),
                            ));
                        }
                        Err(e) => {
                            self.record_event(
                                TaskEventType::ToolCallFailed,
                                serde_json::json!({
                                    "id": tool_call.id,
                                    "error": e.to_string(),
                                }),
                            )
                            .await?;

                            self.messages.push(Message::tool(
                                tool_call.id.clone(),
                                serde_json::json!({"error": e.to_string()}).to_string(),
                            ));
                        }
                    }
                }
            } else {
                // Text-only response
                self.messages.push(Message::assistant(model_response.content.clone()));

                // If tools are not required or tools have completed, proceed to verification
                self.transition(TaskState::Verifying).await?;

                self.record_event(TaskEventType::VerificationStarted, serde_json::json!({}))
                    .await?;

                let (v_result, new_evs) = self
                    .verifier
                    .verify(&self.contract, &self.evidence, None)
                    .await;

                self.evidence.extend(new_evs);

                match v_result.verdict {
                    VerificationVerdict::Pass => {
                        self.record_event(
                            TaskEventType::VerificationPassed,
                            serde_json::to_value(&v_result).unwrap_or_default(),
                        )
                        .await?;
                        self.transition(TaskState::Completed).await?;
                        self.record_event(
                            TaskEventType::TaskCompleted,
                            serde_json::json!({
                                "final_response": model_response.content,
                            }),
                        )
                        .await?;
                    }
                    VerificationVerdict::Fail | VerificationVerdict::Inconclusive => {
                        self.record_event(
                            TaskEventType::VerificationFailed,
                            serde_json::to_value(&v_result).unwrap_or_default(),
                        )
                        .await?;

                        repair_attempts += 1;
                        if repair_attempts <= MAX_REPAIR_ATTEMPTS {
                            self.transition(TaskState::Repairing { attempt: repair_attempts })
                                .await?;
                            self.transition(TaskState::Executing).await?;

                            // Provide repair feedback to model
                            let reasons: Vec<String> = v_result
                                .condition_results
                                .iter()
                                .filter(|c| !c.satisfied)
                                .filter_map(|c| c.reason.clone())
                                .collect();

                            self.messages.push(Message::user(format!(
                                "VERIFICATION FAILED: {}.\nPlease correct the issues using the available tools.",
                                reasons.join("; ")
                            )));
                        } else {
                            let reason = format!("Verification failed after {repair_attempts} repair attempts");
                            if !self.attempt_self_healing(&reason, &mut self_healing_attempts).await? {
                                self.fail(&reason).await?;
                            }
                        }
                    }
                }
            }
        }

        // Record episode into MemoryOS and Session Store if memory manager configured
        if let Some(mem) = &self.memory_manager {
            let session_id = companion_domain::SessionId::from(*self.contract.tenant_id.as_uuid());
            mem.session_store()
                .append_message(session_id.clone(), Message::user(self.contract.objective.clone()))
                .await;

            if let Some(last_assistant_msg) = self.messages.iter().rev().find(|m| m.role == companion_domain::Role::Assistant) {
                mem.session_store()
                    .append_message(session_id, last_assistant_msg.clone())
                    .await;
            }

            let events = self
                .event_store
                .load_events(self.contract.task_id)
                .await
                .unwrap_or_default();
            let _ = mem
                .episodic_recorder()
                .record_task_episode(&self.contract, &self.state, &events)
                .await;
        }

        Ok(self.state.clone())
    }

    async fn attempt_self_healing(
        &mut self,
        reason: &str,
        attempts: &mut u32,
    ) -> Result<bool, RuntimeError> {
        let maybe_sh = self.self_healing_loop.clone();
        if let Some(sh) = maybe_sh {
            let events = self
                .event_store
                .load_events(self.contract.task_id)
                .await
                .unwrap_or_default();
            let temp_state = TaskState::Failed {
                reason: reason.to_string(),
            };

            if let Some(diagnosis) = sh.attempt_healing(
                &self.contract,
                &events,
                &self.evidence,
                &temp_state,
                *attempts,
            ) {
                *attempts += 1;
                self.record_event(
                    TaskEventType::SelfHealingAttempted,
                    serde_json::to_value(&diagnosis).unwrap_or_default(),
                )
                .await?;

                // Transition through Failed -> SelfHealing -> Executing
                self.transition(TaskState::Failed {
                    reason: reason.to_string(),
                })
                .await?;

                self.transition(TaskState::SelfHealing {
                    attempt: *attempts,
                    diagnosis: diagnosis.root_cause.category_label().into(),
                })
                .await?;

                if let Some(plan) = diagnosis.compensation_plan {
                    for action in plan.actions {
                        match action {
                            crate::self_healing::CompensationAction::RetryWithBackoff { delay_ms, .. } => {
                                tokio::time::sleep(Duration::from_millis(delay_ms.min(50))).await;
                            }
                            crate::self_healing::CompensationAction::InjectContext { additional_prompt } => {
                                self.messages.push(Message::user(format!(
                                    "SELF-HEALING ADVICE: {additional_prompt}"
                                )));
                            }
                            _ => {}
                        }
                    }
                }

                self.transition(TaskState::Executing).await?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn fail(&mut self, reason: &str) -> Result<(), RuntimeError> {
        self.transition(TaskState::Failed {
            reason: reason.to_string(),
        })
        .await?;

        self.record_event(
            TaskEventType::TaskFailed,
            serde_json::json!({
                "reason": reason,
            }),
        )
        .await?;

        error!(task_id = %self.contract.task_id, reason = %reason, "task failed");
        Ok(())
    }

    async fn generate_with_retry(&self, request: ModelRequest) -> Result<companion_domain::ModelResponse, ModelError> {
        let mut backoff = Duration::from_millis(500);
        let max_retries = 3;

        for attempt in 1..=max_retries {
            match self.model_router.route(request.clone()).await {
                Ok(resp) => return Ok(resp),
                Err(e) => {
                    warn!(attempt, error = %e, "model generation attempt failed");
                    if attempt == max_retries {
                        return Err(e);
                    }
                    tokio::time::sleep(backoff).await;
                    backoff *= 2;
                }
            }
        }

        Err(ModelError::ProviderError {
            message: "Max retries exceeded".into(),
        })
    }
}
