use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use async_trait::async_trait;
use tracing::{debug, info};

use companion_domain::{
    CapabilityDefinition, CapabilityEnvironment, CapabilityId, CapabilityPermission,
    McpCallToolResult, McpToolInfo, RiskLevel, RuntimeError,
};

use super::protocol::{JsonRpcRequest, JsonRpcResponse};

/// Transport abstraction for communicating with an MCP server (e.g. stdio, WebSocket, or HTTP-SSE).
#[async_trait]
pub trait McpTransport: Send + Sync {
    async fn send_request(&self, request: JsonRpcRequest) -> Result<JsonRpcResponse, RuntimeError>;
}

/// Mock in-memory MCP transport for testing and local integration.
pub struct MockMcpTransport {
    tools: Vec<McpToolInfo>,
    call_handler: Arc<dyn Fn(&str, &serde_json::Value) -> McpCallToolResult + Send + Sync>,
}

impl MockMcpTransport {
    pub fn new(
        tools: Vec<McpToolInfo>,
        handler: impl Fn(&str, &serde_json::Value) -> McpCallToolResult + Send + Sync + 'static,
    ) -> Self {
        Self {
            tools,
            call_handler: Arc::new(handler),
        }
    }
}

#[async_trait]
impl McpTransport for MockMcpTransport {
    async fn send_request(&self, request: JsonRpcRequest) -> Result<JsonRpcResponse, RuntimeError> {
        match request.method.as_str() {
            "initialize" => {
                let res = serde_json::json!({
                    "protocolVersion": "2024-11-05",
                    "serverInfo": {
                        "name": "companion-mock-mcp",
                        "version": "1.0.0"
                    },
                    "capabilities": {
                        "tools": {}
                    }
                });
                Ok(JsonRpcResponse::success(request.id, res))
            }
            "tools/list" => {
                let res = serde_json::json!({
                    "tools": self.tools
                });
                Ok(JsonRpcResponse::success(request.id, res))
            }
            "tools/call" => {
                if let Some(params) = request.params {
                    let tool_name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    let arguments = params.get("arguments").cloned().unwrap_or(serde_json::json!({}));
                    let call_result = (self.call_handler)(tool_name, &arguments);
                    let res = serde_json::to_value(call_result).unwrap_or_default();
                    Ok(JsonRpcResponse::success(request.id, res))
                } else {
                    Ok(JsonRpcResponse::error(request.id, -32602, "Invalid params"))
                }
            }
            _ => Ok(JsonRpcResponse::error(
                request.id,
                -32601,
                format!("Method `{}` not found", request.method),
            )),
        }
    }
}

/// Client for connecting to and executing tools on an external Model Context Protocol server.
pub struct McpClient {
    transport: Arc<dyn McpTransport>,
    next_id: AtomicU64,
}

impl McpClient {
    pub fn new(transport: Arc<dyn McpTransport>) -> Self {
        Self {
            transport,
            next_id: AtomicU64::new(1),
        }
    }

    /// Perform initialize handshake with the MCP server.
    pub async fn initialize(&self) -> Result<serde_json::Value, RuntimeError> {
        let req_id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let params = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "clientInfo": {
                "name": "Companion-Runtime",
                "version": "0.1.0"
            },
            "capabilities": {}
        });

        let req = JsonRpcRequest::new(req_id, "initialize", Some(params));
        let resp = self.transport.send_request(req).await?;

        if let Some(err) = resp.error {
            return Err(RuntimeError::Internal(format!("MCP initialize error: {}", err.message)));
        }

        info!("MCP client handshake successfully initialized");
        Ok(resp.result.unwrap_or_default())
    }

    /// Query tools exposed by the MCP server.
    pub async fn list_tools(&self) -> Result<Vec<McpToolInfo>, RuntimeError> {
        let req_id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let req = JsonRpcRequest::new(req_id, "tools/list", None);
        let resp = self.transport.send_request(req).await?;

        if let Some(err) = resp.error {
            return Err(RuntimeError::Internal(format!("MCP tools/list error: {}", err.message)));
        }

        let result = resp.result.unwrap_or_default();
        let tools: Vec<McpToolInfo> = serde_json::from_value(result.get("tools").cloned().unwrap_or_default())
            .map_err(|e| RuntimeError::Internal(format!("Failed to parse MCP tools/list: {e}")))?;

        debug!(count = tools.len(), "discovered MCP tools");
        Ok(tools)
    }

    /// Invoke a tool on the external MCP server.
    pub async fn call_tool(&self, name: &str, arguments: serde_json::Value) -> Result<McpCallToolResult, RuntimeError> {
        let req_id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let params = serde_json::json!({
            "name": name,
            "arguments": arguments,
        });

        let req = JsonRpcRequest::new(req_id, "tools/call", Some(params));
        let resp = self.transport.send_request(req).await?;

        if let Some(err) = resp.error {
            return Err(RuntimeError::Internal(format!("MCP tools/call error ({}): {}", err.code, err.message)));
        }

        let result: McpCallToolResult = serde_json::from_value(resp.result.unwrap_or_default())
            .map_err(|e| RuntimeError::Internal(format!("Failed to parse MCP tools/call result: {e}")))?;

        Ok(result)
    }

    /// Discover tools from the MCP server and map them to CapabilityDefinition models.
    pub async fn discover_capabilities(&self) -> Result<Vec<CapabilityDefinition>, RuntimeError> {
        let tools = self.list_tools().await?;
        let mut defs = Vec::new();

        for tool in tools {
            let def = CapabilityDefinition {
                id: CapabilityId::new(),
                name: tool.name.clone(),
                description: tool.description.unwrap_or_else(|| format!("MCP tool `{}`", tool.name)),
                parameters: tool.input_schema,
                permissions: vec![CapabilityPermission::Custom(format!("mcp:{}", tool.name))],
                risk_level: RiskLevel::Medium,
                environment: CapabilityEnvironment::Mcp,
                sandbox_policy: None,
                rate_limit: None,
                timeout_ms: 30_000,
            };
            defs.push(def);
        }

        Ok(defs)
    }
}
