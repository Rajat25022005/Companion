use companion_domain::{
    CapabilityDefinition, CapabilityPermission, Constraint, RuntimeError, TaskContract,
};
use tracing::warn;

/// Permission and authorization gate for capability invocations.
pub struct CapabilityPermissionGate;

impl CapabilityPermissionGate {
    /// Authorize a capability invocation against a task contract.
    pub fn check_permission(
        contract: &TaskContract,
        definition: &CapabilityDefinition,
    ) -> Result<(), RuntimeError> {
        // 1. Check if the tool name is in the contract's allowed_tools
        if !contract.allowed_tools.contains(&definition.name) {
            warn!(
                task_id = %contract.task_id,
                capability = %definition.name,
                "tool call rejected: not in task contract allowed_tools"
            );
            return Err(RuntimeError::AuthorizationDenied(format!(
                "Capability `{}` is not permitted by task contract",
                definition.name
            )));
        }

        // 2. Check permissions against constraints
        for perm in &definition.permissions {
            match perm {
                CapabilityPermission::NetworkRead | CapabilityPermission::NetworkWrite => {
                    let has_no_network = contract.constraints.iter().any(|c| matches!(c, Constraint::NoNetwork));
                    if has_no_network {
                        return Err(RuntimeError::AuthorizationDenied(format!(
                            "Capability `{}` requires network access which is prohibited by task constraint",
                            definition.name
                        )));
                    }
                }
                CapabilityPermission::WorkspaceWrite => {
                    let has_read_only = contract.constraints.iter().any(|c| matches!(c, Constraint::ReadOnlyFilesystem));
                    if has_read_only {
                        return Err(RuntimeError::AuthorizationDenied(format!(
                            "Capability `{}` requires filesystem write which is prohibited by read-only constraint",
                            definition.name
                        )));
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }
}
