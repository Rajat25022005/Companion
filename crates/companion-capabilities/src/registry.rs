use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tracing::{debug, info};

use companion_domain::{
    CapabilityDefinition, CapabilityEnvironment, RuntimeError, ToolCall, ToolResult,
};

use crate::mcp::{McpCapability, McpClient};
use crate::ratelimit::RateLimiter;
use crate::wasm::WasmCapability;

/// Trait that all capabilities (tools) implement.
#[async_trait]
pub trait Capability: Send + Sync {
    /// The definition (schema, permissions, risk) of this capability.
    fn definition(&self) -> &CapabilityDefinition;

    /// Execute the capability with the given arguments.
    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult, companion_domain::ToolError>;
}

/// Registry of available capabilities (Native, WASM, MCP, RemoteAgent).
///
/// The runtime queries this to resolve tool names and execute tool calls with rate limiting.
pub struct CapabilityRegistry {
    capabilities: HashMap<String, Arc<dyn Capability>>,
    rate_limiter: RateLimiter,
}

impl CapabilityRegistry {
    pub fn new() -> Self {
        Self {
            capabilities: HashMap::new(),
            rate_limiter: RateLimiter::new(),
        }
    }

    /// Register a capability.
    pub fn register(&mut self, cap: Arc<dyn Capability>) {
        let name = cap.definition().name.clone();
        debug!(capability = %name, environment = ?cap.definition().environment, "registered capability");
        self.capabilities.insert(name, cap);
    }

    /// Register a sandboxed WASM capability.
    pub fn register_wasm(&mut self, definition: CapabilityDefinition, module_bytes: Vec<u8>) {
        let wasm_cap = Arc::new(WasmCapability::new(definition, module_bytes));
        self.register(wasm_cap);
    }

    /// Discover and register all tools exposed by an MCP client.
    pub async fn register_mcp_client(&mut self, client: Arc<McpClient>) -> Result<Vec<String>, RuntimeError> {
        let definitions = client.discover_capabilities().await?;
        let mut registered_names = Vec::new();

        for def in definitions {
            let name = def.name.clone();
            let mcp_cap = Arc::new(McpCapability::new(def, client.clone()));
            self.register(mcp_cap);
            registered_names.push(name);
        }

        info!(count = registered_names.len(), "registered MCP capabilities into registry");
        Ok(registered_names)
    }

    /// Look up a capability by name.
    pub fn get(&self, name: &str) -> Option<&Arc<dyn Capability>> {
        self.capabilities.get(name)
    }

    /// Execute a tool call with rate limiting and circuit breaker protection.
    pub async fn execute(&self, tool_call: &ToolCall) -> Result<ToolResult, RuntimeError> {
        let cap = self
            .capabilities
            .get(&tool_call.name)
            .ok_or_else(|| RuntimeError::CapabilityNotFound(tool_call.name.clone()))?;

        let def = cap.definition();

        // 1. Rate Limit & Circuit Breaker Check
        if let Some(ref policy) = def.rate_limit {
            self.rate_limiter
                .check_and_acquire(&tool_call.name, policy)
                .await
                .map_err(|e| RuntimeError::TaskFailed(format!("rate limit / circuit breaker error: {}", e.message)))?;
        }

        // 2. Execute Capability
        let exec_result = cap.execute(tool_call.arguments.clone()).await;

        // 3. Record outcome for rate limiter / circuit breaker
        if let Some(ref policy) = def.rate_limit {
            self.rate_limiter
                .record_outcome(&tool_call.name, policy, exec_result.is_ok())
                .await;
        }

        exec_result.map_err(|e| RuntimeError::TaskFailed(format!("tool error: {}", e.message)))
    }

    /// List all registered capability definitions.
    pub fn definitions(&self) -> Vec<&CapabilityDefinition> {
        self.capabilities.values().map(|c| c.definition()).collect()
    }

    /// Get definitions for a filtered list of allowed tool names.
    pub fn definitions_for(&self, allowed: &[String]) -> Vec<&CapabilityDefinition> {
        allowed
            .iter()
            .filter_map(|name| self.capabilities.get(name).map(|c| c.definition()))
            .collect()
    }

    /// Filter capability definitions by execution environment.
    pub fn filter_by_environment(&self, env: CapabilityEnvironment) -> Vec<&CapabilityDefinition> {
        self.capabilities
            .values()
            .map(|c| c.definition())
            .filter(|d| d.environment == env)
            .collect()
    }

    pub fn rate_limiter(&self) -> &RateLimiter {
        &self.rate_limiter
    }
}

impl Default for CapabilityRegistry {
    fn default() -> Self {
        Self::new()
    }
}
