use async_trait::async_trait;
use std::time::Instant;

use companion_domain::*;
use crate::registry::Capability;

/// process.execute — runs a shell command and captures output.
pub struct ProcessExecute {
    definition: CapabilityDefinition,
}

impl ProcessExecute {
    pub fn new() -> Self {
        Self {
            definition: CapabilityDefinition::new(
                "process.execute",
                "Execute a shell command and return stdout, stderr, and exit code.",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The command to execute"
                        },
                        "cwd": {
                            "type": "string",
                            "description": "Working directory (optional)"
                        },
                        "timeout_secs": {
                            "type": "integer",
                            "description": "Timeout in seconds (default 30)"
                        }
                    },
                    "required": ["command"]
                }),
                vec![CapabilityPermission::ProcessExecute],
                RiskLevel::High,
            ),
        }
    }
}

#[async_trait]
impl Capability for ProcessExecute {
    fn definition(&self) -> &CapabilityDefinition {
        &self.definition
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult, ToolError> {
        let start = Instant::now();
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError {
                tool_call_id: String::new(),
                message: "missing 'command' parameter".into(),
                retryable: false,
            })?;

        let cwd = args.get("cwd").and_then(|v| v.as_str());
        let timeout_secs = args
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .unwrap_or(30);

        let mut cmd = tokio::process::Command::new("sh");
        cmd.arg("-c").arg(command);
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            cmd.output(),
        )
        .await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let exit_code = output.status.code().unwrap_or(-1);

                Ok(ToolResult {
                    tool_call_id: String::new(),
                    success: exit_code == 0,
                    output: serde_json::json!({
                        "exit_code": exit_code,
                        "stdout": stdout,
                        "stderr": stderr,
                    }),
                    content_hash: None,
                    execution_ms: start.elapsed().as_millis() as u64,
                })
            }
            Ok(Err(e)) => Ok(ToolResult {
                tool_call_id: String::new(),
                success: false,
                output: serde_json::json!({"error": e.to_string()}),
                content_hash: None,
                execution_ms: start.elapsed().as_millis() as u64,
            }),
            Err(_) => Ok(ToolResult {
                tool_call_id: String::new(),
                success: false,
                output: serde_json::json!({
                    "error": format!("command timed out after {timeout_secs}s")
                }),
                content_hash: None,
                execution_ms: start.elapsed().as_millis() as u64,
            }),
        }
    }
}
