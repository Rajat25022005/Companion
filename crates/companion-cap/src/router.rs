use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, oneshot, RwLock};
use tracing::{debug, warn};

use companion_domain::{AgentAddress, AgentId, AgentRole, CapEnvelope, CapMessageId, MessagePattern, RuntimeError};
use crate::mailbox::AgentMailbox;

/// Central router for CAP inter-agent messages.
pub struct CapRouter {
    mailboxes: RwLock<HashMap<AgentId, Arc<AgentMailbox>>>,
    role_registry: RwLock<HashMap<AgentRole, Vec<AgentId>>>,
    pending_replies: RwLock<HashMap<CapMessageId, oneshot::Sender<CapEnvelope>>>,
    _broadcast_tx: broadcast::Sender<CapEnvelope>,
}

impl CapRouter {
    pub fn new() -> Self {
        let (_broadcast_tx, _) = broadcast::channel(100);
        Self {
            mailboxes: RwLock::new(HashMap::new()),
            role_registry: RwLock::new(HashMap::new()),
            pending_replies: RwLock::new(HashMap::new()),
            _broadcast_tx,
        }
    }

    /// Register an agent mailbox with the router.
    pub async fn register_agent(&self, address: AgentAddress, capacity: usize) -> Arc<AgentMailbox> {
        let mailbox = Arc::new(AgentMailbox::new(address.clone(), capacity));
        let mut mbs = self.mailboxes.write().await;
        mbs.insert(address.agent_id, mailbox.clone());

        let mut roles = self.role_registry.write().await;
        roles.entry(address.role.clone()).or_default().push(address.agent_id);

        debug!(agent_id = %address.agent_id, role = %address.role, "registered agent in CAP router");
        mailbox
    }

    /// Unregister an agent.
    pub async fn unregister_agent(&self, address: &AgentAddress) {
        let mut mbs = self.mailboxes.write().await;
        mbs.remove(&address.agent_id);

        let mut roles = self.role_registry.write().await;
        if let Some(list) = roles.get_mut(&address.role) {
            list.retain(|id| id != &address.agent_id);
        }
    }

    /// Find an active agent by role.
    pub async fn find_by_role(&self, role: &AgentRole) -> Option<AgentId> {
        let roles = self.role_registry.read().await;
        roles.get(role).and_then(|list| list.first().copied())
    }

    /// Route a CAP envelope to its recipient.
    pub async fn route(&self, envelope: CapEnvelope) -> Result<(), RuntimeError> {
        debug!(
            msg_id = %envelope.message_id,
            from = %envelope.sender.agent_id,
            to = %envelope.recipient.agent_id,
            "routing CAP message"
        );

        // Check if this is a response answering a pending await
        if let MessagePattern::Response { in_reply_to } = &envelope.pattern {
            let mut pending = self.pending_replies.write().await;
            if let Some(tx) = pending.remove(in_reply_to) {
                let _ = tx.send(envelope.clone());
            }
        }

        // Deliver to direct recipient if registered
        let mbs = self.mailboxes.read().await;
        if let Some(mb) = mbs.get(&envelope.recipient.agent_id) {
            mb.push(envelope).await?;
            return Ok(());
        }

        // Fallback: If recipient agent_id is not directly known, try routing by recipient role
        let roles = self.role_registry.read().await;
        if let Some(agent_ids) = roles.get(&envelope.recipient.role) {
            if let Some(first_id) = agent_ids.first() {
                if let Some(mb) = mbs.get(first_id) {
                    mb.push(envelope).await?;
                    return Ok(());
                }
            }
        }

        warn!(recipient = %envelope.recipient.agent_id, role = %envelope.recipient.role, "recipient mailbox not found");
        Err(RuntimeError::Internal(format!(
            "No active recipient found for address: {} ({})",
            envelope.recipient.agent_id, envelope.recipient.role
        )))
    }

    /// Send a request and wait for the correlated response up to `timeout`.
    pub async fn send_and_await_reply(
        &self,
        envelope: CapEnvelope,
        timeout: Duration,
    ) -> Result<CapEnvelope, RuntimeError> {
        let msg_id = envelope.message_id;
        let (tx, rx) = oneshot::channel();

        {
            let mut pending = self.pending_replies.write().await;
            pending.insert(msg_id, tx);
        }

        self.route(envelope).await?;

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => Err(RuntimeError::Internal("response channel dropped".into())),
            Err(_) => {
                // Cleanup pending reply listener
                let mut pending = self.pending_replies.write().await;
                pending.remove(&msg_id);
                Err(RuntimeError::Internal(format!(
                    "timed out after {}s waiting for CAP response to {}",
                    timeout.as_secs(),
                    msg_id
                )))
            }
        }
    }
}
