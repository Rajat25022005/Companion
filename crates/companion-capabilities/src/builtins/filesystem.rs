use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::time::Instant;

use companion_domain::*;
use crate::registry::Capability;

// ---------------------------------------------------------------------------
// filesystem.read
// ---------------------------------------------------------------------------

pub struct FileRead {
    definition: CapabilityDefinition,
}

impl FileRead {
    pub fn new() -> Self {
        Self {
            definition: CapabilityDefinition::new(
                "filesystem.read",
                "Read the contents of a file at a given path.",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "The file path to read"
                        }
                    },
                    "required": ["path"]
                }),
                vec![CapabilityPermission::WorkspaceRead],
                RiskLevel::Low,
            ),
        }
    }
}

#[async_trait]
impl Capability for FileRead {
    fn definition(&self) -> &CapabilityDefinition {
        &self.definition
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult, ToolError> {
        let start = Instant::now();
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError {
                tool_call_id: String::new(),
                message: "missing 'path' parameter".into(),
                retryable: false,
            })?;

        match tokio::fs::read_to_string(path).await {
            Ok(content) => {
                let hash = format!("{:x}", Sha256::digest(content.as_bytes()));
                Ok(ToolResult {
                    tool_call_id: String::new(),
                    success: true,
                    output: serde_json::json!({
                        "content": content,
                        "size": content.len(),
                    }),
                    content_hash: Some(hash),
                    execution_ms: start.elapsed().as_millis() as u64,
                })
            }
            Err(e) => Ok(ToolResult {
                tool_call_id: String::new(),
                success: false,
                output: serde_json::json!({"error": e.to_string()}),
                content_hash: None,
                execution_ms: start.elapsed().as_millis() as u64,
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// filesystem.write
// ---------------------------------------------------------------------------

pub struct FileWrite {
    definition: CapabilityDefinition,
}

impl FileWrite {
    pub fn new() -> Self {
        Self {
            definition: CapabilityDefinition::new(
                "filesystem.write",
                "Write content to a file at a given path. Creates parent directories if needed.",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "The file path to write to"
                        },
                        "content": {
                            "type": "string",
                            "description": "The content to write"
                        }
                    },
                    "required": ["path", "content"]
                }),
                vec![CapabilityPermission::WorkspaceWrite],
                RiskLevel::Medium,
            ),
        }
    }
}

#[async_trait]
impl Capability for FileWrite {
    fn definition(&self) -> &CapabilityDefinition {
        &self.definition
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult, ToolError> {
        let start = Instant::now();

        let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
            ToolError {
                tool_call_id: String::new(),
                message: "Missing 'path' argument".into(),
                retryable: false,
            }
        })?;

        let content = args.get("content").and_then(|v| v.as_str()).ok_or_else(|| {
            ToolError {
                tool_call_id: String::new(),
                message: "Missing 'content' argument".into(),
                retryable: false,
            }
        })?;

        // Ensure parent directories exist
        if let Some(parent) = std::path::Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent).await.map_err(|e| {
                    ToolError {
                        tool_call_id: String::new(),
                        message: format!("Failed to create parent directories: {e}"),
                        retryable: false,
                    }
                })?;
            }
        }

        tokio::fs::write(path, content).await.map_err(|e| {
            ToolError {
                tool_call_id: String::new(),
                message: format!("Failed to write file '{path}': {e}"),
                retryable: false,
            }
        })?;

        // Hash the written content
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        let hash = format!("{:x}", hasher.finalize());

        let elapsed = start.elapsed().as_millis() as u64;

        Ok(ToolResult {
            tool_call_id: String::new(),
            success: true,
            output: serde_json::json!({
                "path": path,
                "bytes_written": content.len(),
                "content_hash": hash,
            }),
            content_hash: Some(hash),
            execution_ms: elapsed,
        })
    }
}

// ---------------------------------------------------------------------------
// filesystem.list
// ---------------------------------------------------------------------------

pub struct FileList {
    definition: CapabilityDefinition,
}

impl FileList {
    pub fn new() -> Self {
        Self {
            definition: CapabilityDefinition::new(
                "filesystem.list",
                "List files and directories at a given path.",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "The directory path to list"
                        }
                    },
                    "required": ["path"]
                }),
                vec![CapabilityPermission::WorkspaceRead],
                RiskLevel::Low,
            ),
        }
    }
}

#[async_trait]
impl Capability for FileList {
    fn definition(&self) -> &CapabilityDefinition {
        &self.definition
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult, ToolError> {
        let start = Instant::now();
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or(".");

        match tokio::fs::read_dir(path).await {
            Ok(mut entries) => {
                let mut items = Vec::new();
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let is_dir = entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
                    items.push(serde_json::json!({
                        "name": name,
                        "is_directory": is_dir,
                    }));
                }
                Ok(ToolResult {
                    tool_call_id: String::new(),
                    success: true,
                    output: serde_json::json!({
                        "entries": items,
                        "count": items.len(),
                    }),
                    content_hash: None,
                    execution_ms: start.elapsed().as_millis() as u64,
                })
            }
            Err(e) => Ok(ToolResult {
                tool_call_id: String::new(),
                success: false,
                output: serde_json::json!({"error": e.to_string()}),
                content_hash: None,
                execution_ms: start.elapsed().as_millis() as u64,
            }),
        }
    }
}
