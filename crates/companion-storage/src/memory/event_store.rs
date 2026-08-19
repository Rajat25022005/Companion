use std::collections::HashMap;
use std::sync::RwLock;
use async_trait::async_trait;

use companion_domain::{EventStoreError, TaskId};
use companion_events::{EventStore, TaskEvent};

/// In-memory implementation of the EventStore trait for testing and local development.
pub struct InMemoryEventStore {
    events: RwLock<HashMap<TaskId, Vec<TaskEvent>>>,
}

impl InMemoryEventStore {
    pub fn new() -> Self {
        Self {
            events: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryEventStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EventStore for InMemoryEventStore {
    async fn append(&self, event: TaskEvent) -> Result<(), EventStoreError> {
        let mut map = self.events.write().map_err(|e| EventStoreError::AppendFailed(e.to_string()))?;
        let list = map.entry(event.task_id).or_default();
        list.push(event);
        Ok(())
    }

    async fn append_batch(&self, events: Vec<TaskEvent>) -> Result<(), EventStoreError> {
        let mut map = self.events.write().map_err(|e| EventStoreError::AppendFailed(e.to_string()))?;
        for event in events {
            let list = map.entry(event.task_id).or_default();
            list.push(event);
        }
        Ok(())
    }

    async fn load_events(&self, task_id: TaskId) -> Result<Vec<TaskEvent>, EventStoreError> {
        let map = self.events.read().map_err(|e| EventStoreError::LoadFailed(e.to_string()))?;
        Ok(map.get(&task_id).cloned().unwrap_or_default())
    }

    async fn load_events_since(
        &self,
        task_id: TaskId,
        after_sequence: i64,
    ) -> Result<Vec<TaskEvent>, EventStoreError> {
        let map = self.events.read().map_err(|e| EventStoreError::LoadFailed(e.to_string()))?;
        let events = map.get(&task_id).cloned().unwrap_or_default();
        Ok(events.into_iter().filter(|e| e.sequence > after_sequence).collect())
    }

    async fn latest_sequence(&self, task_id: TaskId) -> Result<Option<i64>, EventStoreError> {
        let map = self.events.read().map_err(|e| EventStoreError::LoadFailed(e.to_string()))?;
        Ok(map.get(&task_id).and_then(|list| list.last().map(|e| e.sequence)))
    }
}
