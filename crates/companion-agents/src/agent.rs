use std::sync::Arc;
use tracing::info;

use companion_cap::AgentMailbox;
use companion_domain::{
    AgentAddress, RuntimeError, TaskId, TaskState,
};
use companion_runtime::RuntimeEngine;

/// An active specialized agent worker in the Companion platform.
pub struct AgentInstance {
    pub address: AgentAddress,
    pub system_prompt: String,
    pub allowed_tools: Vec<String>,
    pub mailbox: Arc<AgentMailbox>,
    pub engine: Arc<RuntimeEngine>,
}

impl AgentInstance {
    pub fn new(
        address: AgentAddress,
        system_prompt: String,
        allowed_tools: Vec<String>,
        mailbox: Arc<AgentMailbox>,
        engine: Arc<RuntimeEngine>,
    ) -> Self {
        Self {
            address,
            system_prompt,
            allowed_tools,
            mailbox,
            engine,
        }
    }

    /// Factory: Coordinator Persona
    pub fn coordinator(mailbox: Arc<AgentMailbox>, engine: Arc<RuntimeEngine>) -> Self {
        let address = mailbox.address.clone();
        let prompt = "You are the Coordinator Agent. You break down complex goals into structured \
                      DAG workflows, coordinate team execution, delegate subtasks to Architect, Engineer, \
                      and Reviewer agents, and ensure high-quality delivery."
            .into();
        let tools = vec!["filesystem.read".into(), "filesystem.list".into()];
        Self::new(address, prompt, tools, mailbox, engine)
    }

    /// Factory: Architect Persona
    pub fn architect(mailbox: Arc<AgentMailbox>, engine: Arc<RuntimeEngine>) -> Self {
        let address = mailbox.address.clone();
        let prompt = "You are the Architect Agent. You design system architectures, specify interfaces, \
                      choose data schemas, create technical plans, and guide engineering implementations."
            .into();
        let tools = vec!["filesystem.read".into(), "filesystem.list".into()];
        Self::new(address, prompt, tools, mailbox, engine)
    }

    /// Factory: Engineer Persona
    pub fn engineer(mailbox: Arc<AgentMailbox>, engine: Arc<RuntimeEngine>) -> Self {
        let address = mailbox.address.clone();
        let prompt = "You are the Engineer Agent. You implement robust, production-grade code, \
                      write files, fix bugs, and execute local tools with high precision."
            .into();
        let tools = vec![
            "filesystem.read".into(),
            "filesystem.write".into(),
            "filesystem.list".into(),
            "process.execute".into(),
        ];
        Self::new(address, prompt, tools, mailbox, engine)
    }

    /// Factory: Reviewer Persona
    pub fn reviewer(mailbox: Arc<AgentMailbox>, engine: Arc<RuntimeEngine>) -> Self {
        let address = mailbox.address.clone();
        let prompt = "You are the Reviewer / Critic Agent. You inspect code for correctness, security, \
                      and performance. You run unit tests and benchmarks to verify deterministic evidence."
            .into();
        let tools = vec![
            "filesystem.read".into(),
            "filesystem.list".into(),
            "process.execute".into(),
        ];
        Self::new(address, prompt, tools, mailbox, engine)
    }

    /// Execute a task assigned to this agent through the runtime engine.
    pub async fn execute_task(
        &self,
        prompt: &str,
        workspace_root: Option<String>,
    ) -> Result<(TaskId, TaskState, serde_json::Value), RuntimeError> {
        info!(
            agent = %self.address.agent_id,
            role = %self.address.role,
            "executing assigned task"
        );

        let (task_id, state, contract) = self
            .engine
            .submit_and_run(prompt, None, None, workspace_root)
            .await?;

        let output = serde_json::json!({
            "agent_id": self.address.agent_id.to_string(),
            "role": self.address.role.to_string(),
            "final_state": state.to_string(),
            "objective": contract.objective,
        });

        Ok((task_id, state, output))
    }
}
