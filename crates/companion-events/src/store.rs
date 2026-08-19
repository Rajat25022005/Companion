use async_trait::async_trait;

use companion_domain::{EventStoreError, TaskId};

use crate::TaskEvent;

/// Durable event store trait.
///
/// Implementations must guarantee:
/// - Events are appended atomically
/// - Sequence numbers are unique per task
/// - Events are returned in sequence order
#[async_trait]
pub trait EventStore: Send + Sync {
    /// Append a single event. Fails if sequence number conflicts.
    async fn append(&self, event: TaskEvent) -> Result<(), EventStoreError>;

    /// Append multiple events atomically.
    async fn append_batch(&self, events: Vec<TaskEvent>) -> Result<(), EventStoreError>;

    /// Load all events for a task, ordered by sequence.
    async fn load_events(&self, task_id: TaskId) -> Result<Vec<TaskEvent>, EventStoreError>;

    /// Load events after a given sequence number (for resumption).
    async fn load_events_since(
        &self,
        task_id: TaskId,
        after_sequence: i64,
    ) -> Result<Vec<TaskEvent>, EventStoreError>;

    /// Get the latest sequence number for a task.
    async fn latest_sequence(&self, task_id: TaskId) -> Result<Option<i64>, EventStoreError>;
}
