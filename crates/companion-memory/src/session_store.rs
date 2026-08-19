use std::collections::HashMap;
use tokio::sync::RwLock;
use companion_domain::{MemoryItem, MemoryTier, Message, SessionId};

/// L1 Session Store: maintains conversational turn history and active session transcripts.
#[derive(Debug, Default)]
pub struct SessionStore {
    sessions: RwLock<HashMap<SessionId, Vec<Message>>>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
        }
    }

    /// Append a turn message to an active session.
    pub async fn append_message(&self, session_id: SessionId, message: Message) {
        let mut map = self.sessions.write().await;
        map.entry(session_id).or_default().push(message);
    }

    /// Get all messages in a session.
    pub async fn get_messages(&self, session_id: &SessionId) -> Vec<Message> {
        let map = self.sessions.read().await;
        map.get(session_id).cloned().unwrap_or_default()
    }

    /// Get the most recent N messages from the session (sliding window).
    pub async fn get_recent_turns(&self, session_id: &SessionId, max_turns: usize) -> Vec<Message> {
        let map = self.sessions.read().await;
        if let Some(msgs) = map.get(session_id) {
            let start = msgs.len().saturating_sub(max_turns);
            msgs[start..].to_vec()
        } else {
            Vec::new()
        }
    }

    /// Estimate total tokens across all messages in the session (rough 4 chars per token).
    pub async fn estimate_session_tokens(&self, session_id: &SessionId) -> usize {
        let map = self.sessions.read().await;
        if let Some(msgs) = map.get(session_id) {
            let chars: usize = msgs
                .iter()
                .map(|m| m.content.len())
                .sum();
            chars / 4
        } else {
            0
        }
    }

    /// Compact old session turns into a single summary message when the session exceeds `threshold_turns`.
    /// Retains `keep_recent` most recent turns verbatim and prepends a summary turn.
    pub async fn compact_session(
        &self,
        session_id: &SessionId,
        threshold_turns: usize,
        keep_recent: usize,
    ) -> Option<MemoryItem> {
        let mut map = self.sessions.write().await;
        let msgs = map.get_mut(session_id)?;

        if msgs.len() <= threshold_turns {
            return None;
        }

        let split_idx = msgs.len().saturating_sub(keep_recent);
        let older_turns: Vec<Message> = msgs.drain(..split_idx).collect();

        if older_turns.is_empty() {
            return None;
        }

        // Create summary bullets from older turns
        let mut summary_lines = Vec::new();
        for msg in &older_turns {
            let role_str = match msg.role {
                companion_domain::Role::User => "User",
                companion_domain::Role::Assistant => "Assistant",
                companion_domain::Role::System => "System",
                companion_domain::Role::Tool => "Tool",
            };
            let content_preview = msg
                .content
                .lines()
                .next()
                .unwrap_or("")
                .trim();
            if !content_preview.is_empty() {
                summary_lines.push(format!("- {}: {}", role_str, content_preview));
            }
        }

        let summary_text = format!(
            "[Compacted Previous Session History ({} turns)]:\n{}",
            older_turns.len(),
            summary_lines.join("\n")
        );

        // Prepend summary message to active session
        msgs.insert(0, Message::system(summary_text.clone()));

        // Create L1 session memory item
        let item = MemoryItem::new(MemoryTier::Session, summary_text).with_metadata(serde_json::json!({
            "session_id": session_id.to_string(),
            "compacted_turns": older_turns.len(),
        }));

        Some(item)
    }

    /// Clear session history.
    pub async fn clear_session(&self, session_id: &SessionId) {
        let mut map = self.sessions.write().await;
        map.remove(session_id);
    }
}
