use chrono::Utc;
use companion_domain::{
    CapabilityRequirement, CompletionCondition, Constraint, CorrelationId,
    IntentClassification, Mode, RiskLevel, TaskBudget, TaskContract, TaskId,
    TenantId, WorkspaceId,
};

/// Compiles an IntentClassification and user parameters into a strict TaskContract.
#[derive(Debug, Clone, Default)]
pub struct ContractCompiler;

impl ContractCompiler {
    pub fn new() -> Self {
        Self
    }

    /// Compile intent into a rigid TaskContract.
    pub fn compile(
        &self,
        task_id: TaskId,
        tenant_id: TenantId,
        workspace_id: WorkspaceId,
        correlation_id: CorrelationId,
        user_input: &str,
        intent: IntentClassification,
        workspace_path: Option<String>,
    ) -> TaskContract {
        let mut required_capabilities = Vec::new();
        let mut allowed_tools = Vec::new();
        let mut completion_conditions = Vec::new();
        let mut constraints = Vec::new();
        let mut risk_level = RiskLevel::None;

        if let Some(path) = workspace_path {
            constraints.push(Constraint::WorkspaceRoot { path });
        }

        let mut budget = TaskBudget::default();

        match intent.mode_profile.primary {
            Mode::Build | Mode::Code => {
                risk_level = RiskLevel::Medium;
                budget.max_turns = 60;
                budget.max_tool_calls = 150;
                budget.max_time_secs = 900;
                budget.max_tokens = 600_000;

                required_capabilities.push(CapabilityRequirement {
                    capability: "filesystem.write".into(),
                    required: true,
                });
                allowed_tools.push("filesystem.read".into());
                allowed_tools.push("filesystem.write".into());
                allowed_tools.push("filesystem.list".into());
                allowed_tools.push("process.execute".into());

                // Derive completion condition from intent
                let paths = self.extract_potential_paths(&intent.message);
                if !paths.is_empty() {
                    completion_conditions.push(CompletionCondition::FilesExist { paths });
                } else {
                    completion_conditions.push(CompletionCondition::ToolInvoked {
                        capability: "filesystem.write".into(),
                    });
                }
            }
            Mode::Debug => {
                risk_level = RiskLevel::High;
                budget.max_turns = 50;
                budget.max_tool_calls = 120;
                budget.max_time_secs = 900;
                budget.max_tokens = 500_000;

                required_capabilities.push(CapabilityRequirement {
                    capability: "process.execute".into(),
                    required: true,
                });
                allowed_tools.push("filesystem.read".into());
                allowed_tools.push("filesystem.write".into());
                allowed_tools.push("process.execute".into());

                completion_conditions.push(CompletionCondition::ToolInvoked {
                    capability: "process.execute".into(),
                });
            }
            Mode::Ask | Mode::Research | Mode::Summary => {
                risk_level = RiskLevel::None;
                budget.max_turns = 15;
                budget.max_tool_calls = 30;
                allowed_tools.push("filesystem.read".into());
                allowed_tools.push("filesystem.list".into());
                completion_conditions.push(CompletionCondition::ModelResponseProduced);
            }
            _ => {
                if intent.mode_profile.tools_required {
                    risk_level = RiskLevel::Medium;
                    budget.max_turns = 40;
                    budget.max_tool_calls = 80;
                    allowed_tools.push("filesystem.read".into());
                    allowed_tools.push("filesystem.write".into());
                    allowed_tools.push("filesystem.list".into());
                    allowed_tools.push("process.execute".into());
                    completion_conditions.push(CompletionCondition::ToolInvoked {
                        capability: "filesystem.write".into(),
                    });
                } else {
                    completion_conditions.push(CompletionCondition::ModelResponseProduced);
                }
            }
        }

        TaskContract {
            task_id,
            tenant_id,
            workspace_id,
            correlation_id,
            workflow_id: None,
            goal_id: None,
            parent_task_id: None,
            user_input: user_input.to_string(),
            objective: intent.message.clone(),
            mode_profile: intent.mode_profile,
            required_capabilities,
            allowed_tools,
            completion_conditions,
            constraints,
            risk_level,
            budget,
            created_at: Utc::now(),
        }
    }

    fn extract_potential_paths(&self, message: &str) -> Vec<String> {
        let mut paths = Vec::new();
        for word in message.split_whitespace() {
            let clean = word.trim_matches(|c| c == '\'' || c == '"' || c == ',' || c == '`');
            if clean.contains('.') && !clean.ends_with('.') && !clean.starts_with("http") {
                paths.push(clean.to_string());
            }
        }
        paths
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IntentParser;

    #[test]
    fn test_compile_build_contract() {
        let parser = IntentParser::new();
        let compiler = ContractCompiler::new();
        let input = "#build Create hello.txt with content";
        let intent = parser.parse(input);

        let contract = compiler.compile(
            TaskId::new(),
            TenantId::new(),
            WorkspaceId::new(),
            CorrelationId::new(),
            input,
            intent,
            None,
        );

        assert_eq!(contract.risk_level, RiskLevel::Medium);
        assert!(contract.allowed_tools.contains(&"filesystem.write".to_string()));
        assert!(!contract.completion_conditions.is_empty());
    }
}
