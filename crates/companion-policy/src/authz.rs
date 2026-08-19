use companion_domain::{TaskContract, ToolCall};
use tracing::{debug, warn};

/// Authorization gate — checks if a tool call is allowed by the task contract.
pub struct AuthorizationGate;

impl AuthorizationGate {
    pub fn new() -> Self {
        Self
    }

    /// Check whether a tool call is authorized under the task contract.
    pub fn authorize(
        &self,
        contract: &TaskContract,
        tool_call: &ToolCall,
    ) -> AuthorizationDecision {
        if contract.allowed_tools.contains(&tool_call.name) {
            debug!(tool = %tool_call.name, "tool authorized");
            AuthorizationDecision::Allowed
        } else {
            warn!(tool = %tool_call.name, "tool denied — not in allowed_tools");
            AuthorizationDecision::Denied {
                tool: tool_call.name.clone(),
                reason: format!(
                    "Tool '{}' is not in the allowed tools for this task. Allowed: {:?}",
                    tool_call.name, contract.allowed_tools
                ),
            }
        }
    }
}

/// Result of an authorization check.
#[derive(Debug, Clone)]
pub enum AuthorizationDecision {
    Allowed,
    Denied { tool: String, reason: String },
}
