use std::sync::Arc;
use companion_capabilities::CapabilityRegistry;
use companion_context::ContextCompiler;
use companion_domain::{CorrelationId, RuntimeError, TaskContract, TaskId, TaskState, TenantId, WorkspaceId};
use companion_events::EventStore;
use companion_memory::MemoryManager;
use companion_models::ModelRouter;
use companion_policy::{AuthorizationGate, ToolIntentMonitor};
use companion_storage::TaskStore;

use crate::contract_compiler::ContractCompiler;
use crate::execution_loop::ExecutionLoop;
use crate::intent_parser::IntentParser;
use crate::verifier::Verifier;

pub struct RuntimeEngine {
    model_router: Arc<ModelRouter>,
    capability_registry: Arc<CapabilityRegistry>,
    event_store: Arc<dyn EventStore>,
    task_store: Arc<dyn TaskStore>,
    intent_parser: Arc<IntentParser>,
    contract_compiler: Arc<ContractCompiler>,
    policy_monitor: Arc<ToolIntentMonitor>,
    authz_gate: Arc<AuthorizationGate>,
    verifier: Arc<Verifier>,
    context_compiler: Option<Arc<ContextCompiler>>,
    memory_manager: Option<Arc<MemoryManager>>,
    skill_registry: Option<Arc<companion_skills::SkillRegistry>>,
    policy_evaluator: Option<Arc<companion_policy::PolicyEvaluator>>,
    hitl_gate: Option<Arc<companion_policy::HitlApprovalGate>>,
    self_healing_loop: Option<Arc<crate::self_healing::SelfHealingLoop>>,
    profile_manager: Option<Arc<companion_profile::ProfileManager>>,
}

impl RuntimeEngine {
    pub fn new(
        model_router: Arc<ModelRouter>,
        capability_registry: Arc<CapabilityRegistry>,
        event_store: Arc<dyn EventStore>,
        task_store: Arc<dyn TaskStore>,
    ) -> Self {
        Self {
            model_router,
            capability_registry,
            event_store,
            task_store,
            intent_parser: Arc::new(IntentParser::new()),
            contract_compiler: Arc::new(ContractCompiler::new()),
            policy_monitor: Arc::new(ToolIntentMonitor::new()),
            authz_gate: Arc::new(AuthorizationGate::new()),
            verifier: Arc::new(Verifier::new()),
            context_compiler: None,
            memory_manager: None,
            skill_registry: None,
            policy_evaluator: None,
            hitl_gate: None,
            self_healing_loop: None,
            profile_manager: None,
        }
    }

    pub fn with_context_compiler(mut self, compiler: Arc<ContextCompiler>) -> Self {
        self.context_compiler = Some(compiler);
        self
    }

    pub fn with_memory_manager(mut self, memory_manager: Arc<MemoryManager>) -> Self {
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

    /// Compile a task contract without executing it immediately.
    pub fn compile_contract(
        &self,
        input: &str,
        tenant_id: Option<TenantId>,
        workspace_id: Option<WorkspaceId>,
        workspace_path: Option<String>,
    ) -> (TaskId, TaskContract) {
        let task_id = TaskId::new();
        let correlation_id = CorrelationId::new();
        let tenant = tenant_id.unwrap_or_else(TenantId::new);
        let workspace = workspace_id.unwrap_or_else(WorkspaceId::new);

        let intent = self.intent_parser.parse(input);
        let contract = self.contract_compiler.compile(
            task_id,
            tenant,
            workspace,
            correlation_id,
            input,
            intent,
            workspace_path,
        );
        (task_id, contract)
    }

    /// Execute a compiled contract to completion.
    pub async fn run_contract(&self, contract: TaskContract) -> Result<TaskState, RuntimeError> {
        let mut exec = ExecutionLoop::new(
            contract,
            self.model_router.clone(),
            self.capability_registry.clone(),
            self.event_store.clone(),
            self.task_store.clone(),
            self.policy_monitor.clone(),
            self.authz_gate.clone(),
            self.verifier.clone(),
        );

        if let Some(compiler) = &self.context_compiler {
            exec = exec.with_context_compiler(compiler.clone());
        }
        if let Some(mem) = &self.memory_manager {
            exec = exec.with_memory_manager(mem.clone());
        }
        if let Some(reg) = &self.skill_registry {
            exec = exec.with_skill_registry(reg.clone());
        }
        if let Some(eval) = &self.policy_evaluator {
            exec = exec.with_policy_evaluator(eval.clone());
        }
        if let Some(gate) = &self.hitl_gate {
            exec = exec.with_hitl_gate(gate.clone());
        }
        if let Some(sh) = &self.self_healing_loop {
            exec = exec.with_self_healing_loop(sh.clone());
        }
        if let Some(pm) = &self.profile_manager {
            exec = exec.with_profile_manager(pm.clone());
        }

        exec.run().await
    }

    /// Submit a task input, compile its contract, and execute it to completion.
    pub async fn submit_and_run(
        &self,
        input: &str,
        tenant_id: Option<TenantId>,
        workspace_id: Option<WorkspaceId>,
        workspace_path: Option<String>,
    ) -> Result<(TaskId, TaskState, TaskContract), RuntimeError> {
        let (task_id, contract) = self.compile_contract(input, tenant_id, workspace_id, workspace_path);
        let final_state = self.run_contract(contract.clone()).await?;
        Ok((task_id, final_state, contract))
    }

    pub fn event_store(&self) -> &Arc<dyn EventStore> {
        &self.event_store
    }

    pub fn task_store(&self) -> &Arc<dyn TaskStore> {
        &self.task_store
    }

    pub fn model_router(&self) -> &Arc<ModelRouter> {
        &self.model_router
    }

    pub fn memory_manager(&self) -> Option<&Arc<MemoryManager>> {
        self.memory_manager.as_ref()
    }

    pub fn context_compiler(&self) -> Option<&Arc<ContextCompiler>> {
        self.context_compiler.as_ref()
    }

    pub fn policy_evaluator(&self) -> Option<&Arc<companion_policy::PolicyEvaluator>> {
        self.policy_evaluator.as_ref()
    }

    pub fn hitl_gate(&self) -> Option<&Arc<companion_policy::HitlApprovalGate>> {
        self.hitl_gate.as_ref()
    }

    pub fn self_healing_loop(&self) -> Option<&Arc<crate::self_healing::SelfHealingLoop>> {
        self.self_healing_loop.as_ref()
    }

    pub fn profile_manager(&self) -> Option<&Arc<companion_profile::ProfileManager>> {
        self.profile_manager.as_ref()
    }
}
