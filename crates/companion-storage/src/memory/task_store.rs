use std::collections::HashMap;
use std::sync::RwLock;
use async_trait::async_trait;

use companion_domain::{TaskContract, TaskId, TaskState, TaskStoreError};
use crate::postgres::TaskStore;

/// In-memory implementation of the TaskStore trait for testing and development.
pub struct InMemoryTaskStore {
    tasks: RwLock<HashMap<TaskId, (TaskState, TaskContract)>>,
}

impl InMemoryTaskStore {
    pub fn new() -> Self {
        Self {
            tasks: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryTaskStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TaskStore for InMemoryTaskStore {
    async fn save(
        &self,
        task_id: TaskId,
        state: &TaskState,
        contract: &TaskContract,
    ) -> Result<(), TaskStoreError> {
        let mut map = self.tasks.write().map_err(|e| TaskStoreError::SaveFailed(e.to_string()))?;
        map.insert(task_id, (state.clone(), contract.clone()));
        Ok(())
    }

    async fn update_state(
        &self,
        task_id: TaskId,
        state: &TaskState,
    ) -> Result<(), TaskStoreError> {
        let mut map = self.tasks.write().map_err(|e| TaskStoreError::SaveFailed(e.to_string()))?;
        if let Some((s, _)) = map.get_mut(&task_id) {
            *s = state.clone();
            Ok(())
        } else {
            Err(TaskStoreError::NotFound(task_id.to_string()))
        }
    }

    async fn get_state(&self, task_id: TaskId) -> Result<TaskState, TaskStoreError> {
        let map = self.tasks.read().map_err(|e| TaskStoreError::ConnectionError(e.to_string()))?;
        map.get(&task_id)
            .map(|(s, _)| s.clone())
            .ok_or_else(|| TaskStoreError::NotFound(task_id.to_string()))
    }

    async fn get_contract(&self, task_id: TaskId) -> Result<TaskContract, TaskStoreError> {
        let map = self.tasks.read().map_err(|e| TaskStoreError::ConnectionError(e.to_string()))?;
        map.get(&task_id)
            .map(|(_, c)| c.clone())
            .ok_or_else(|| TaskStoreError::NotFound(task_id.to_string()))
    }
}
