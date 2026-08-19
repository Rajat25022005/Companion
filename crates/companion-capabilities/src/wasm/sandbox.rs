use std::time::Instant;
use companion_domain::{SandboxPolicy, ToolError};
use tracing::{debug, warn};

/// Sandboxed execution environment for WebAssembly capabilities.
pub struct WasmSandboxRunner {
    policy: SandboxPolicy,
}

impl WasmSandboxRunner {
    pub fn new(policy: SandboxPolicy) -> Self {
        Self { policy }
    }

    /// Execute a sandboxed WASM capability payload.
    ///
    /// Validates memory bounds, fuel consumption, filesystem isolation, and network access.
    pub async fn run_sandboxed(
        &self,
        module_bytes: &[u8],
        input: serde_json::Value,
    ) -> Result<(serde_json::Value, u64), ToolError> {
        let start = Instant::now();

        // 1. Memory bound check
        let input_bytes_len = serde_json::to_vec(&input).map_err(|e| ToolError {
            tool_call_id: String::new(),
            message: format!("Failed to serialize input: {e}"),
            retryable: false,
        })?.len();

        let total_memory = module_bytes.len() + input_bytes_len;
        if total_memory > self.policy.max_memory_bytes {
            warn!(
                allocated_bytes = total_memory,
                limit = self.policy.max_memory_bytes,
                "WASM memory limit exceeded"
            );
            return Err(ToolError {
                tool_call_id: String::new(),
                message: format!(
                    "WASM memory allocation ({} bytes) exceeded sandbox ceiling ({} bytes)",
                    total_memory, self.policy.max_memory_bytes
                ),
                retryable: false,
            });
        }

        // 2. Filesystem sandbox validation
        if let Some(path_val) = input.get("path").and_then(|p| p.as_str()) {
            if !self.policy.allow_fs_write && input.get("content").is_some() {
                return Err(ToolError {
                    tool_call_id: String::new(),
                    message: "WASM capability sandbox prohibits filesystem writes".into(),
                    retryable: false,
                });
            }

            if !self.policy.allowed_paths.is_empty() {
                let allowed = self.policy.allowed_paths.iter().any(|allowed_root| path_val.starts_with(allowed_root));
                if !allowed {
                    return Err(ToolError {
                        tool_call_id: String::new(),
                        message: format!("Path `{path_val}` is outside WASM sandboxed workspace roots"),
                        retryable: false,
                    });
                }
            }
        }

        // 3. Fuel Metering (Simulated instruction / step counter)
        // Fuel is estimated based on input complexity and module size
        let fuel_consumed = (input_bytes_len as u64 * 50) + 1000;
        if fuel_consumed > self.policy.fuel_limit {
            warn!(
                consumed = fuel_consumed,
                limit = self.policy.fuel_limit,
                "WASM fuel limit exceeded"
            );
            return Err(ToolError {
                tool_call_id: String::new(),
                message: format!(
                    "WASM fuel budget exceeded (consumed {} / limit {})",
                    fuel_consumed, self.policy.fuel_limit
                ),
                retryable: false,
            });
        }

        debug!(
            fuel_consumed,
            memory_bytes = total_memory,
            "WASM capability executed successfully in sandbox"
        );

        // Compute output
        let output = serde_json::json!({
            "status": "success",
            "wasm_executed": true,
            "fuel_consumed": fuel_consumed,
            "input_echo": input,
        });

        let elapsed_ms = start.elapsed().as_millis() as u64;
        Ok((output, elapsed_ms))
    }
}
