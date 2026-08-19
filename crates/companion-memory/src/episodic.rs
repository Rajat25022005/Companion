use std::sync::Arc;
use companion_domain::{MemoryItem, MemoryTier, RuntimeError, TaskContract, TaskState};
use companion_events::TaskEvent;
use crate::embeddings::EmbeddingProvider;
use crate::vector_store::VectorStore;

pub struct EpisodicRecorder {
    embedder: Arc<dyn EmbeddingProvider>,
    vector_store: Arc<VectorStore>,
}

impl EpisodicRecorder {
    pub fn new(embedder: Arc<dyn EmbeddingProvider>, vector_store: Arc<VectorStore>) -> Self {
        Self {
            embedder,
            vector_store,
        }
    }

    /// Record a completed task and its events into episodic memory.
    pub async fn record_task_episode(
        &self,
        contract: &TaskContract,
        final_state: &TaskState,
        events: &[TaskEvent],
    ) -> Result<MemoryItem, RuntimeError> {
        let tools_used: Vec<String> = events
            .iter()
            .filter(|e| e.event_type == companion_events::TaskEventType::ToolCallStarted)
            .filter_map(|e| e.payload.get("name").and_then(|v| v.as_str()).map(String::from))
            .collect();

        let summary = format!(
            "Task: {}\nObjective: {}\nOutcome: {}\nTools Invoked: {:?}\nEvents Count: {}",
            contract.user_input,
            contract.objective,
            final_state,
            tools_used,
            events.len()
        );

        let embedding = self.embedder.embed(&summary).await?;

        let mut item = MemoryItem::new(MemoryTier::Episodic, summary)
            .with_embedding(embedding)
            .with_metadata(serde_json::json!({
                "task_id": contract.task_id.to_string(),
                "mode": format!("{:?}", contract.mode_profile.primary),
                "state": final_state.to_string(),
                "tools_used": tools_used,
            }));

        if let TaskState::Completed = final_state {
            item = item.with_importance(1.2);
        }

        self.vector_store.insert(item.clone()).await;
        Ok(item)
    }
}
