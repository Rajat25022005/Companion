use std::sync::Arc;
use std::time::Instant;
use async_trait::async_trait;
use sha2::{Digest, Sha256};

use companion_domain::{
    CapabilityDefinition, CapabilityEnvironment, ToolError, ToolResult,
};

use crate::registry::Capability;
use super::client::McpClient;

/// A capability adapter that delegates tool execution to an external MCP server.
pub struct McpCapability {
    definition: CapabilityDefinition,
    client: Arc<McpClient>,
}

impl McpCapability {
    pub fn new(definition: CapabilityDefinition, client: Arc<McpClient>) -> Self {
        let mut def = definition;
        def.environment = CapabilityEnvironment::Mcp;
        Self {
            definition: def,
            client,
        }
    }
}

#[async_trait]
impl Capability for McpCapability {
    fn definition(&self) -> &CapabilityDefinition {
        &self.definition
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult, ToolError> {
        let start = Instant::now();

        let mcp_res = self
            .client
            .call_tool(&self.definition.name, args)
            .await
            .map_err(|e| ToolError {
                tool_call_id: String::new(),
                message: format!("MCP execution error: {e}"),
                retryable: true,
            })?;

        let elapsed_ms = start.elapsed().as_millis() as u64;

        if mcp_res.is_error {
            let err_msg = mcp_res
                .content
                .iter()
                .filter_map(|c| c.text.clone())
                .collect::<Vec<_>>()
                .join("\n");

            return Err(ToolError {
                tool_call_id: String::new(),
                message: if err_msg.is_empty() {
                    "MCP tool returned error".into()
                } else {
                    err_msg
                },
                retryable: false,
            });
        }

        let output_json = serde_json::to_value(&mcp_res.content).unwrap_or(serde_json::json!({
            "status": "success",
            "content": mcp_res.content
        }));

        // Compute output hash for Evidence Ledger
        let output_bytes = serde_json::to_vec(&output_json).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(&output_bytes);
        let content_hash = format!("{:x}", hasher.finalize());

        Ok(ToolResult {
            tool_call_id: String::new(),
            success: true,
            output: output_json,
            content_hash: Some(content_hash),
            execution_ms: elapsed_ms,
        })
    }
}
