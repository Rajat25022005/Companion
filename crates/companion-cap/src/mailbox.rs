use tokio::sync::{mpsc, RwLock};
use companion_domain::{AgentAddress, CapEnvelope, RuntimeError};

/// Inbound mailbox channel for an agent.
pub struct AgentMailbox {
    pub address: AgentAddress,
    sender: mpsc::Sender<CapEnvelope>,
    receiver: RwLock<mpsc::Receiver<CapEnvelope>>,
}

impl AgentMailbox {
    pub fn new(address: AgentAddress, capacity: usize) -> Self {
        let (sender, receiver) = mpsc::channel(capacity);
        Self {
            address,
            sender,
            receiver: RwLock::new(receiver),
        }
    }

    /// Send a message into this agent's mailbox.
    pub async fn push(&self, envelope: CapEnvelope) -> Result<(), RuntimeError> {
        self.sender
            .send(envelope)
            .await
            .map_err(|e| RuntimeError::Internal(format!("mailbox channel send failed: {e}")))
    }

    /// Pull the next message from this agent's mailbox.
    pub async fn pop(&self) -> Option<CapEnvelope> {
        let mut rx = self.receiver.write().await;
        rx.recv().await
    }

    /// Try to pull a message without blocking.
    pub async fn try_pop(&self) -> Option<CapEnvelope> {
        let mut rx = self.receiver.write().await;
        rx.try_recv().ok()
    }
}
