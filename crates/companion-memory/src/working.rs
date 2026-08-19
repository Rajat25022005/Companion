use std::collections::HashMap;
use tokio::sync::RwLock;
use companion_domain::{MemoryItem, MemoryTier, TaskId};

/// L0 Working Memory: in-memory transient scratchpad for active task turns and reasoning notes.
#[derive(Debug, Default)]
pub struct WorkingMemory {
    /// Task-scoped scratchpad entries: task_id -> list of working memory items
    scratchpads: RwLock<HashMap<TaskId, Vec<MemoryItem>>>,
}

impl WorkingMemory {
    pub fn new() -> Self {
        Self {
            scratchpads: RwLock::new(HashMap::new()),
        }
    }

    /// Add a transient thought, scratchpad note, or intermediate plan to active working memory.
    pub async fn push_scratchpad(&self, task_id: TaskId, note: impl Into<String>) -> MemoryItem {
        let content = note.into();
        let item = MemoryItem::new(MemoryTier::Working, content);

        let mut map = self.scratchpads.write().await;
        map.entry(task_id).or_default().push(item.clone());
        item
    }

    /// Retrieve all working memory entries for a task.
    pub async fn get_scratchpad(&self, task_id: &TaskId) -> Vec<MemoryItem> {
        let map = self.scratchpads.read().await;
        map.get(task_id).cloned().unwrap_or_default()
    }

    /// Clear scratchpad when a task completes or is reset.
    pub async fn clear_task(&self, task_id: &TaskId) {
        let mut map = self.scratchpads.write().await;
        map.remove(task_id);
    }

    /// Format working memory notes as a Markdown section for prompt injection.
    pub async fn format_working_notes(&self, task_id: &TaskId) -> Option<String> {
        let items = self.get_scratchpad(task_id).await;
        if items.is_empty() {
            return None;
        }

        let mut lines = Vec::new();
        for item in items {
            lines.push(format!("- {}", item.content));
        }
        Some(format!("### Working Memory (Scratchpad):\n{}", lines.join("\n")))
    }
}
