use std::collections::HashMap;
use tokio::sync::RwLock;
use tracing::{debug, warn};

use companion_domain::{
    CompiledContext, ContextBudget, ContextGrant, ContextRequest, ContextSources,
    DataSensitivity, GrantId, RuntimeError, TaskId,
};

use crate::compiler::ContextCompiler;

/// Context Broker: manages least-privilege context grants and mediates context retrieval for agents/tasks.
#[derive(Debug, Default)]
pub struct ContextBroker {
    grants: RwLock<HashMap<GrantId, ContextGrant>>,
    task_grants: RwLock<HashMap<TaskId, GrantId>>,
}

impl ContextBroker {
    pub fn new() -> Self {
        Self {
            grants: RwLock::new(HashMap::new()),
            task_grants: RwLock::new(HashMap::new()),
        }
    }

    /// Issue a new least-privilege ContextGrant.
    pub async fn issue_grant(
        &self,
        task_id: TaskId,
        sensitivity_ceiling: DataSensitivity,
        token_budget: usize,
    ) -> ContextGrant {
        let grant = ContextGrant::new(task_id, sensitivity_ceiling, token_budget);
        let grant_id = grant.grant_id;

        {
            let mut map = self.grants.write().await;
            map.insert(grant_id, grant.clone());
        }
        {
            let mut task_map = self.task_grants.write().await;
            task_map.insert(task_id, grant_id);
        }

        debug!(grant_id = %grant_id, task_id = %task_id, "issued context grant");
        grant
    }

    /// Register a custom ContextGrant.
    pub async fn register_grant(&self, grant: ContextGrant) {
        let grant_id = grant.grant_id;
        let task_id = grant.task_id;
        {
            let mut map = self.grants.write().await;
            map.insert(grant_id, grant);
        }
        {
            let mut task_map = self.task_grants.write().await;
            task_map.insert(task_id, grant_id);
        }
    }

    /// Retrieve an active grant by GrantId.
    pub async fn get_grant(&self, grant_id: &GrantId) -> Option<ContextGrant> {
        let map = self.grants.read().await;
        map.get(grant_id).cloned()
    }

    /// Get active grant for a task.
    pub async fn get_task_grant(&self, task_id: &TaskId) -> Option<ContextGrant> {
        let task_map = self.task_grants.read().await;
        let grant_id = task_map.get(task_id)?;
        self.get_grant(grant_id).await
    }

    /// Revoke a grant.
    pub async fn revoke_grant(&self, grant_id: &GrantId) {
        let mut map = self.grants.write().await;
        if let Some(grant) = map.remove(grant_id) {
            let mut task_map = self.task_grants.write().await;
            task_map.remove(&grant.task_id);
            warn!(grant_id = %grant_id, "revoked context grant");
        }
    }

    /// Handle a ContextRequest: evaluates permissions, bounds token budget, and compiles context.
    pub async fn request_context(
        &self,
        request: &ContextRequest,
        sources: &ContextSources,
        compiler: &ContextCompiler,
    ) -> Result<CompiledContext, RuntimeError> {
        let grant = self.get_task_grant(&request.task_id).await;

        let token_limit = request
            .max_tokens
            .or_else(|| grant.as_ref().map(|g| g.token_budget))
            .unwrap_or(4096);

        let budget = ContextBudget::for_total_tokens(token_limit);
        compiler.compile(sources, &budget, grant.as_ref()).await
    }
}
