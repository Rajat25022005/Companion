use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

use companion_cap::CapRouter;
use companion_domain::{AgentAddress, AgentId, AgentRole, RuntimeError};
use companion_runtime::RuntimeEngine;

use crate::agent::AgentInstance;

/// Container managing an active team of collaborating specialized agents.
pub struct AgentTeam {
    agents: RwLock<HashMap<AgentId, Arc<AgentInstance>>>,
    role_map: RwLock<HashMap<AgentRole, AgentId>>,
    router: Arc<CapRouter>,
    engine: Arc<RuntimeEngine>,
}

impl AgentTeam {
    pub fn new(router: Arc<CapRouter>, engine: Arc<RuntimeEngine>) -> Self {
        Self {
            agents: RwLock::new(HashMap::new()),
            role_map: RwLock::new(HashMap::new()),
            router,
            engine,
        }
    }

    /// Spawn standard 4-agent team (Coordinator, Architect, Engineer, Reviewer).
    pub async fn spawn_default_team(&self) -> Result<(), RuntimeError> {
        info!("Spawning standard Companion agent team...");

        self.spawn_role(AgentRole::Coordinator).await?;
        self.spawn_role(AgentRole::Architect).await?;
        self.spawn_role(AgentRole::Engineer).await?;
        self.spawn_role(AgentRole::Reviewer).await?;

        info!("Companion agent team ready.");
        Ok(())
    }

    /// Spawn a single specialized agent.
    pub async fn spawn_role(&self, role: AgentRole) -> Result<Arc<AgentInstance>, RuntimeError> {
        let address = AgentAddress::new(role.clone());
        let mailbox = self.router.register_agent(address.clone(), 50).await;

        let instance = match &role {
            AgentRole::Coordinator => AgentInstance::coordinator(mailbox, self.engine.clone()),
            AgentRole::Architect => AgentInstance::architect(mailbox, self.engine.clone()),
            AgentRole::Engineer => AgentInstance::engineer(mailbox, self.engine.clone()),
            AgentRole::Reviewer => AgentInstance::reviewer(mailbox, self.engine.clone()),
            AgentRole::Researcher => {
                let prompt = "You are the Researcher Agent. You search and analyze technical context.".into();
                let tools = vec!["filesystem.read".into(), "filesystem.list".into()];
                AgentInstance::new(address.clone(), prompt, tools, mailbox, self.engine.clone())
            }
            AgentRole::Custom(name) => {
                let prompt = format!("You are a specialist Agent for {name}.");
                let tools = vec!["filesystem.read".into(), "filesystem.list".into()];
                AgentInstance::new(address.clone(), prompt, tools, mailbox, self.engine.clone())
            }
        };

        let instance = Arc::new(instance);
        let mut map = self.agents.write().await;
        map.insert(address.agent_id, instance.clone());

        let mut rmap = self.role_map.write().await;
        rmap.insert(role, address.agent_id);

        Ok(instance)
    }

    /// Get agent instance by role.
    pub async fn get_by_role(&self, role: &AgentRole) -> Option<Arc<AgentInstance>> {
        let rmap = self.role_map.read().await;
        if let Some(agent_id) = rmap.get(role) {
            let map = self.agents.read().await;
            map.get(agent_id).cloned()
        } else {
            None
        }
    }

    /// Get agent instance by agent_id.
    pub async fn get_by_id(&self, agent_id: &AgentId) -> Option<Arc<AgentInstance>> {
        let map = self.agents.read().await;
        map.get(agent_id).cloned()
    }

    pub fn router(&self) -> &Arc<CapRouter> {
        &self.router
    }
}
