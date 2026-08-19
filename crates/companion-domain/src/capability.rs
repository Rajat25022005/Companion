use serde::{Deserialize, Serialize};

use crate::ids::CapabilityId;

// ---------------------------------------------------------------------------
// Capability Environment & Sandboxing
// ---------------------------------------------------------------------------

/// Execution environment for a capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityEnvironment {
    /// In-process native Rust implementation.
    #[default]
    Native,
    /// Isolated WebAssembly runtime with fuel and memory limits.
    Wasm,
    /// External Model Context Protocol (MCP) server adapter over stdio/HTTP.
    Mcp,
    /// Remote agent capability via Agent-to-Agent (A2A) protocol.
    RemoteAgent,
}

/// Sandboxing policy and resource constraints for isolated capability execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxPolicy {
    /// Maximum memory limit in bytes (default 64MB).
    pub max_memory_bytes: usize,
    /// Maximum fuel/instruction budget to prevent infinite execution.
    pub fuel_limit: u64,
    /// Whether outbound network access is allowed.
    pub allow_network: bool,
    /// Whether filesystem writes are permitted.
    pub allow_fs_write: bool,
    /// Allowed filesystem paths/roots.
    #[serde(default)]
    pub allowed_paths: Vec<String>,
}

impl Default for SandboxPolicy {
    fn default() -> Self {
        Self {
            max_memory_bytes: 64 * 1024 * 1024,
            fuel_limit: 10_000_000,
            allow_network: false,
            allow_fs_write: true,
            allowed_paths: Vec::new(),
        }
    }
}

/// Rate limiting and circuit breaker policy for a capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitPolicy {
    /// Max allowed invocations per minute.
    pub requests_per_minute: u32,
    /// Max concurrent in-flight invocations.
    pub max_concurrent: u32,
    /// Consecutive failure threshold to trip circuit breaker.
    pub circuit_breaker_threshold: u32,
    /// Cooldown window in seconds before attempting half-open probe.
    pub cooldown_seconds: u64,
}

impl Default for RateLimitPolicy {
    fn default() -> Self {
        Self {
            requests_per_minute: 120,
            max_concurrent: 10,
            circuit_breaker_threshold: 5,
            cooldown_seconds: 30,
        }
    }
}

// ---------------------------------------------------------------------------
// Capability Permissions
// ---------------------------------------------------------------------------

/// Permission required to invoke a capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityPermission {
    /// Read from workspace filesystem.
    WorkspaceRead,
    /// Write to workspace filesystem.
    WorkspaceWrite,
    /// Execute processes.
    ProcessExecute,
    /// Read from network.
    NetworkRead,
    /// Write to network / external systems.
    NetworkWrite,
    /// Custom permission.
    Custom(String),
}

// ---------------------------------------------------------------------------
// Capability Definition
// ---------------------------------------------------------------------------

fn default_timeout_ms() -> u64 {
    30_000
}

/// The schema and metadata for a registered capability (tool).
///
/// Every tool—native Rust, WASM, MCP, or remote—resolves to this contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityDefinition {
    /// Unique ID for this capability.
    pub id: CapabilityId,

    /// Human-readable name (e.g., "filesystem.write").
    pub name: String,

    /// Description of what this capability does.
    pub description: String,

    /// JSON Schema for the tool's input parameters.
    pub parameters: serde_json::Value,

    /// Permissions required to invoke this capability.
    pub permissions: Vec<CapabilityPermission>,

    /// Risk level of invoking this capability.
    pub risk_level: crate::task::RiskLevel,

    /// Execution environment.
    #[serde(default)]
    pub environment: CapabilityEnvironment,

    /// Sandboxing constraints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_policy: Option<SandboxPolicy>,

    /// Rate limiting configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<RateLimitPolicy>,

    /// Execution timeout in milliseconds.
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

impl CapabilityDefinition {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
        permissions: Vec<CapabilityPermission>,
        risk_level: crate::task::RiskLevel,
    ) -> Self {
        Self {
            id: CapabilityId::new(),
            name: name.into(),
            description: description.into(),
            parameters,
            permissions,
            risk_level,
            environment: CapabilityEnvironment::Native,
            sandbox_policy: None,
            rate_limit: None,
            timeout_ms: default_timeout_ms(),
        }
    }

    pub fn with_environment(mut self, env: CapabilityEnvironment) -> Self {
        self.environment = env;
        self
    }

    pub fn with_sandbox_policy(mut self, policy: SandboxPolicy) -> Self {
        self.sandbox_policy = Some(policy);
        self
    }

    pub fn with_rate_limit(mut self, policy: RateLimitPolicy) -> Self {
        self.rate_limit = Some(policy);
        self
    }

    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }
}

// ---------------------------------------------------------------------------
// Tool Call / Result
// ---------------------------------------------------------------------------

/// A tool call as proposed by the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Unique ID for this tool call instance.
    pub id: String,

    /// The capability/tool name being invoked.
    pub name: String,

    /// Arguments as JSON.
    pub arguments: serde_json::Value,
}

/// The result of executing a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// The tool call ID this result corresponds to.
    pub tool_call_id: String,

    /// Whether the tool executed successfully.
    pub success: bool,

    /// The output of the tool as JSON.
    pub output: serde_json::Value,

    /// Content hash of the output (for evidence).
    pub content_hash: Option<String>,

    /// Execution time in milliseconds.
    pub execution_ms: u64,
}

/// Error from tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolError {
    /// The tool call ID.
    pub tool_call_id: String,

    /// Error message.
    pub message: String,

    /// Whether this error is retryable.
    pub retryable: bool,
}

// ---------------------------------------------------------------------------
// Tool Definition for Model Context
// ---------------------------------------------------------------------------

/// A tool definition as presented to the model in its context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Tool name.
    pub name: String,

    /// Tool description.
    pub description: String,

    /// Parameter schema (JSON Schema).
    pub parameters: serde_json::Value,
}

impl From<&CapabilityDefinition> for ToolDefinition {
    fn from(cap: &CapabilityDefinition) -> Self {
        Self {
            name: cap.name.clone(),
            description: cap.description.clone(),
            parameters: cap.parameters.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// MCP Protocol Wire Types
// ---------------------------------------------------------------------------

/// Tool specification exposed by an external Model Context Protocol server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

/// Parameter block for an MCP tools/call request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpCallToolParams {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<serde_json::Value>,
}

/// Output item in an MCP tools/call response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolResultContent {
    #[serde(rename = "type")]
    pub content_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// Result payload from an MCP tools/call execution.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpCallToolResult {
    pub content: Vec<McpToolResultContent>,
    #[serde(default)]
    pub is_error: bool,
}
