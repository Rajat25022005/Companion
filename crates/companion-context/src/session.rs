use std::sync::Arc;
use companion_domain::{
    CompiledContext, ContextBudget, ContextSources, Message, RuntimeError, SessionId,
};
use companion_memory::{MemoryManager, SessionStore};

use crate::compiler::ContextCompiler;

/// Session Manager: manages long-running conversation context and automatic compaction.
pub struct SessionManager {
    session_store: Arc<SessionStore>,
    compiler: Arc<ContextCompiler>,
    memory_manager: Arc<MemoryManager>,
    max_session_tokens: usize,
}

impl SessionManager {
    pub fn new(compiler: Arc<ContextCompiler>, memory_manager: Arc<MemoryManager>) -> Self {
        Self {
            session_store: memory_manager.session_store().clone(),
            compiler,
            memory_manager,
            max_session_tokens: 3000,
        }
    }

    pub fn with_max_session_tokens(mut self, max_tokens: usize) -> Self {
        self.max_session_tokens = max_tokens;
        self
    }

    /// Append a turn message to the session.
    /// If session tokens exceed `max_session_tokens`, automatically compact older history.
    pub async fn add_message(&self, session_id: SessionId, message: Message) {
        self.session_store.append_message(session_id, message).await;

        // Check if compaction is needed
        let current_tokens = self.session_store.estimate_session_tokens(&session_id).await;
        if current_tokens > self.max_session_tokens {
            if let Some(compacted_item) = self
                .session_store
                .compact_session(&session_id, 10, 4)
                .await
            {
                // Also store the compacted episode into long-term episodic memory
                let _ = self.memory_manager.remember_record(compacted_item).await;
            }
        }
    }

    /// Compile a full prompt context for the session.
    pub async fn compile_session_context(
        &self,
        session_id: &SessionId,
        query: &str,
        budget: &ContextBudget,
    ) -> Result<CompiledContext, RuntimeError> {
        let session_turns = self.session_store.get_messages(session_id).await;
        let recalled_memories = self.memory_manager.recall(query, 5, 0.1).await?;

        let sources = ContextSources {
            identity_policy: Some("You are Companion in an active continuous session.".into()),
            session_turns,
            recalled_memories,
            user_input: Some(query.to_string()),
            ..Default::default()
        };

        self.compiler.compile(&sources, budget, None).await
    }

    pub fn session_store(&self) -> &Arc<SessionStore> {
        &self.session_store
    }
}
