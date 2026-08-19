use companion_domain::{Evidence, ModelResponse, TaskContract};
use tracing::{debug, warn};

/// Result of evaluating a model turn against the task contract.
#[derive(Debug, Clone)]
pub enum TurnVerdict {
    /// Turn is acceptable — tools were called, already executed, or not required.
    Accepted,
    /// Turn is rejected — required tools were not called.
    Rejected {
        reason: String,
        missing_capabilities: Vec<String>,
    },
}

/// Monitors whether model turns satisfy task contract requirements.
///
/// If a task requires tool invocations (e.g., `#build`) but no tools have been
/// executed yet and the model only emits text, the turn is rejected.
pub struct ToolIntentMonitor;

impl ToolIntentMonitor {
    pub fn new() -> Self {
        Self
    }

    /// Evaluate whether a model's response satisfies the task contract given past evidence.
    pub fn evaluate_turn(
        &self,
        contract: &TaskContract,
        response: &ModelResponse,
        evidence: &[Evidence],
    ) -> TurnVerdict {
        // If no tools are required, any response is acceptable
        if !contract.mode_profile.tools_required {
            debug!("turn accepted: no tools required");
            return TurnVerdict::Accepted;
        }

        // If the model proposed tool calls in this turn, accept
        if response.has_tool_calls() {
            debug!(
                tool_count = response.tool_calls.len(),
                "turn accepted: model proposed tool calls"
            );
            return TurnVerdict::Accepted;
        }

        // If required tools have already been invoked and produced evidence, accept text response for verification
        let already_invoked = !evidence.is_empty();
        if already_invoked {
            debug!("turn accepted: required tools have been executed in prior turns");
            return TurnVerdict::Accepted;
        }

        // Contract requires tools but model emitted only text without any prior tool invocations — reject
        let required: Vec<String> = contract
            .required_capabilities
            .iter()
            .filter(|c| c.required)
            .map(|c| c.capability.clone())
            .collect();

        warn!(
            missing = ?required,
            "turn rejected: required tools not called"
        );

        TurnVerdict::Rejected {
            reason: "Task requires tool invocations, but model produced only text before calling any tools. \
                     Please use the available tools to complete the task."
                .into(),
            missing_capabilities: required,
        }
    }
}
