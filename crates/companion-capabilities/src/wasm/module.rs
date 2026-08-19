use async_trait::async_trait;
use sha2::{Digest, Sha256};

use companion_domain::{CapabilityDefinition, CapabilityEnvironment, SandboxPolicy, ToolError, ToolResult};

use crate::registry::Capability;
use super::sandbox::WasmSandboxRunner;

/// A capability backed by an isolated WebAssembly sandbox module.
pub struct WasmCapability {
    definition: CapabilityDefinition,
    module_bytes: Vec<u8>,
    runner: WasmSandboxRunner,
}

impl WasmCapability {
    pub fn new(
        definition: CapabilityDefinition,
        module_bytes: Vec<u8>,
    ) -> Self {
        let policy = definition.sandbox_policy.clone().unwrap_or_default();
        let mut def = definition;
        def.environment = CapabilityEnvironment::Wasm;
        def.sandbox_policy = Some(policy.clone());

        Self {
            definition: def,
            module_bytes,
            runner: WasmSandboxRunner::new(policy),
        }
    }

    pub fn with_policy(mut self, policy: SandboxPolicy) -> Self {
        self.definition.sandbox_policy = Some(policy.clone());
        self.runner = WasmSandboxRunner::new(policy);
        self
    }
}

#[async_trait]
impl Capability for WasmCapability {
    fn definition(&self) -> &CapabilityDefinition {
        &self.definition
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult, ToolError> {
        let (output, elapsed_ms) = self.runner.run_sandboxed(&self.module_bytes, args).await?;

        // Calculate output SHA256 content hash for Evidence Ledger
        let output_bytes = serde_json::to_vec(&output).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(&output_bytes);
        let hash = format!("{:x}", hasher.finalize());

        Ok(ToolResult {
            tool_call_id: String::new(),
            success: true,
            output,
            content_hash: Some(hash),
            execution_ms: elapsed_ms,
        })
    }
}
