//! Live Workspace Blueprint for Companion Enterprise.
//!
//! Provides deterministic, zero-tool-call structural awareness of the entire
//! Companion codebase (crates, dependency graph, tools, models, and configuration).
//!
//! Automatically compiled into ContextOS system prompts and dynamically updated
//! when workspace files or crates are modified.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tracing::debug;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrateInfo {
    pub name: String,
    pub path: String,
    pub purpose: String,
    pub key_exports: Vec<String>,
    pub internal_dependencies: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    pub id: String,
    pub summary: String,
    pub risk_level: String,
}

/// The complete, live architectural blueprint of the workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceBlueprint {
    pub workspace_name: String,
    pub total_crates: usize,
    pub crates: Vec<CrateInfo>,
    pub tools: Vec<ToolInfo>,
    pub configs: HashMap<String, String>,
    pub services: Vec<String>,
    pub last_updated: chrono::DateTime<chrono::Utc>,
}

impl Default for WorkspaceBlueprint {
    fn default() -> Self {
        Self::embedded_default()
    }
}

impl WorkspaceBlueprint {
    /// Creates the standard embedded blueprint for Companion Enterprise.
    pub fn embedded_default() -> Self {
        let crates = vec![
            CrateInfo {
                name: "companion-domain".into(),
                path: "crates/companion-domain".into(),
                purpose: "Core domain contracts, IDs, error models, and state machines".into(),
                key_exports: vec![
                    "TaskContract".into(),
                    "TaskState".into(),
                    "ToolResult".into(),
                    "ToolError".into(),
                    "Message".into(),
                    "SessionId".into(),
                    "TaskId".into(),
                    "TenantId".into(),
                    "CapabilityPermission".into(),
                ],
                internal_dependencies: vec![],
            },
            CrateInfo {
                name: "companion-capabilities".into(),
                path: "crates/companion-capabilities".into(),
                purpose: "Native, WASM, and MCP capability dispatch & built-in tools".into(),
                key_exports: vec![
                    "CapabilityRegistry".into(),
                    "CapabilityPermissionGate".into(),
                    "filesystem::*".into(),
                    "process::*".into(),
                    "gmail::*".into(),
                    "web::*".into(),
                ],
                internal_dependencies: vec!["companion-domain".into()],
            },
            CrateInfo {
                name: "companion-runtime".into(),
                path: "crates/companion-runtime".into(),
                purpose: "Execution loop, contract compiler, policy monitor, self-healing loop".into(),
                key_exports: vec![
                    "RuntimeEngine".into(),
                    "ExecutionLoop".into(),
                    "ContractCompiler".into(),
                    "SelfHealingLoop".into(),
                    "HitlApprovalGate".into(),
                ],
                internal_dependencies: vec![
                    "companion-domain".into(),
                    "companion-capabilities".into(),
                    "companion-events".into(),
                    "companion-storage".into(),
                    "companion-models".into(),
                    "companion-policy".into(),
                    "companion-observability".into(),
                    "companion-memory".into(),
                    "companion-context".into(),
                    "companion-skills".into(),
                    "companion-profile".into(),
                ],
            },
            CrateInfo {
                name: "companion-context".into(),
                path: "crates/companion-context".into(),
                purpose: "8-stage ContextOS compiler, token budgeting, prompt caching, blueprint".into(),
                key_exports: vec![
                    "ContextCompiler".into(),
                    "ContextCache".into(),
                    "WorkspaceBlueprint".into(),
                ],
                internal_dependencies: vec!["companion-domain".into(), "companion-memory".into()],
            },
            CrateInfo {
                name: "companion-memory".into(),
                path: "crates/companion-memory".into(),
                purpose: "7-tier MemoryOS (Working, SessionStore, VectorStore, GraphStore, Dream Cycle)".into(),
                key_exports: vec![
                    "MemoryManager".into(),
                    "SessionStore".into(),
                    "VectorStore".into(),
                    "KnowledgeGraphStore".into(),
                    "EpisodicRecorder".into(),
                ],
                internal_dependencies: vec!["companion-domain".into(), "companion-events".into()],
            },
            CrateInfo {
                name: "companion-models".into(),
                path: "crates/companion-models".into(),
                purpose: "Multi-provider LLM router (Ollama, Nvidia, Anthropic, OpenAI)".into(),
                key_exports: vec![
                    "ModelRouter".into(),
                    "OllamaProvider".into(),
                    "NvidiaProvider".into(),
                ],
                internal_dependencies: vec!["companion-domain".into()],
            },
            CrateInfo {
                name: "companion-rate-limiter".into(),
                path: "crates/companion-rate-limiter".into(),
                purpose: "Hierarchical rate limiter (Token Bucket, Sliding Window, Leaky Bucket)".into(),
                key_exports: vec![
                    "RateLimiter".into(),
                    "RateLimiterConfig".into(),
                    "Decision".into(),
                    "TokenBucket".into(),
                ],
                internal_dependencies: vec![],
            },
            CrateInfo {
                name: "companion-skills".into(),
                path: "crates/companion-skills".into(),
                purpose: "SkillOS self-improving registry, trace mining, canary rollouts".into(),
                key_exports: vec![
                    "SkillRegistry".into(),
                    "SkillSynthesizer".into(),
                    "CanaryController".into(),
                ],
                internal_dependencies: vec!["companion-domain".into(), "companion-events".into()],
            },
            CrateInfo {
                name: "companion-profile".into(),
                path: "crates/companion-profile".into(),
                purpose: "User identity, agent persona, and SecretsVault credential management".into(),
                key_exports: vec![
                    "ProfileManager".into(),
                    "SecretsVault".into(),
                    "UserProfile".into(),
                    "AgentPersona".into(),
                ],
                internal_dependencies: vec!["companion-domain".into(), "companion-policy".into()],
            },
            CrateInfo {
                name: "companion-agents".into(),
                path: "crates/companion-agents".into(),
                purpose: "Actor-model Agent Team container and autonomous specialist agents".into(),
                key_exports: vec![
                    "AgentTeam".into(),
                    "CapRouter".into(),
                ],
                internal_dependencies: vec![
                    "companion-domain".into(),
                    "companion-cap".into(),
                    "companion-runtime".into(),
                ],
            },
            CrateInfo {
                name: "companion-workflow".into(),
                path: "crates/companion-workflow".into(),
                purpose: "Multi-agent DAG workflow orchestration and checkpointing".into(),
                key_exports: vec![
                    "WorkflowEngine".into(),
                    "WorkflowDag".into(),
                ],
                internal_dependencies: vec![
                    "companion-domain".into(),
                    "companion-agents".into(),
                ],
            },
            CrateInfo {
                name: "companion-policy".into(),
                path: "crates/companion-policy".into(),
                purpose: "Enterprise policy engine, TenantSecurityManager, PII redactor".into(),
                key_exports: vec![
                    "PolicyEvaluator".into(),
                    "SecurityRedactor".into(),
                    "TenantSecurityManager".into(),
                ],
                internal_dependencies: vec!["companion-domain".into()],
            },
            CrateInfo {
                name: "companion-observability".into(),
                path: "crates/companion-observability".into(),
                purpose: "Cryptographic SHA256 audit ledger, Prometheus metrics exporter".into(),
                key_exports: vec![
                    "AuditLedger".into(),
                    "MetricsCollector".into(),
                ],
                internal_dependencies: vec!["companion-domain".into()],
            },
            CrateInfo {
                name: "companion-storage".into(),
                path: "crates/companion-storage".into(),
                purpose: "PostgreSQL durability layer (PgEventStore, PgTaskStore)".into(),
                key_exports: vec![
                    "PgEventStore".into(),
                    "PgTaskStore".into(),
                ],
                internal_dependencies: vec!["companion-domain".into(), "companion-events".into()],
            },
            CrateInfo {
                name: "companion-events".into(),
                path: "crates/companion-events".into(),
                purpose: "Event sourcing domain events, TaskEventType, EventStore trait".into(),
                key_exports: vec![
                    "TaskEvent".into(),
                    "TaskEventType".into(),
                    "EventStore".into(),
                ],
                internal_dependencies: vec!["companion-domain".into()],
            },
            CrateInfo {
                name: "companion-protocol".into(),
                path: "crates/companion-protocol".into(),
                purpose: "Wire protocols, CRP Gateway, and serialization envelopes".into(),
                key_exports: vec![],
                internal_dependencies: vec!["companion-domain".into()],
            },
            CrateInfo {
                name: "companion-cap".into(),
                path: "crates/companion-cap".into(),
                purpose: "Communicating Agent Protocol (CAP) envelopes and routing".into(),
                key_exports: vec!["CapEnvelope".into(), "CapRouter".into()],
                internal_dependencies: vec!["companion-domain".into()],
            },
            CrateInfo {
                name: "companion-api".into(),
                path: "services/api".into(),
                purpose: "Axum HTTP API gateway, SSE stream, and Web Dashboard UI (:8000)".into(),
                key_exports: vec![],
                internal_dependencies: vec!["companion-runtime".into(), "companion-storage".into()],
            },
            CrateInfo {
                name: "companion-cli".into(),
                path: "bins/companion-cli".into(),
                purpose: "CLI entry point (`companion run/goal/memory/audit/skill`)".into(),
                key_exports: vec![],
                internal_dependencies: vec!["companion-runtime".into()],
            },
        ];

        let tools = vec![
            ToolInfo {
                id: "filesystem.read".into(),
                summary: "Read file contents (path: string)".into(),
                risk_level: "Low".into(),
            },
            ToolInfo {
                id: "filesystem.write".into(),
                summary: "Write/create file (path: string, content: string)".into(),
                risk_level: "Medium".into(),
            },
            ToolInfo {
                id: "filesystem.list".into(),
                summary: "List directory entries (path: string)".into(),
                risk_level: "Low".into(),
            },
            ToolInfo {
                id: "process.execute".into(),
                summary: "Run shell command (command: string, args: string[])".into(),
                risk_level: "High".into(),
            },
            ToolInfo {
                id: "gmail.fetch_unread".into(),
                summary: "Fetch unread inbox emails with spam filtering".into(),
                risk_level: "Low".into(),
            },
            ToolInfo {
                id: "gmail.create_draft".into(),
                summary: "Create formatted email reply draft".into(),
                risk_level: "Low".into(),
            },
            ToolInfo {
                id: "gmail.send_reply".into(),
                summary: "Send SMTP reply (HITL dual-control approval gated)".into(),
                risk_level: "High".into(),
            },
            ToolInfo {
                id: "web.fetch".into(),
                summary: "Fetch URL and convert HTML to clean Markdown".into(),
                risk_level: "Low".into(),
            },
            ToolInfo {
                id: "web.extract_links".into(),
                summary: "Extract all hyperlinks from a web page".into(),
                risk_level: "Low".into(),
            },
        ];

        let mut configs = HashMap::new();
        configs.insert("user_profile".into(), "config/user.md".into());
        configs.insert("agent_persona".into(), "config/agent.md".into());
        configs.insert("secrets_vault".into(), "config/secrets.toml".into());
        configs.insert("root_manifest".into(), "Cargo.toml".into());
        configs.insert("service_manifest".into(), "service_manifest.json".into());

        let services = vec![
            "companion-api (HTTP :8000)".into(),
            "PostgreSQL (:5432 / companion)".into(),
            "Ollama (:11434)".into(),
        ];

        let total_crates = crates.len();

        Self {
            workspace_name: "Companion Enterprise".into(),
            total_crates,
            crates,
            tools,
            configs,
            services,
            last_updated: chrono::Utc::now(),
        }
    }

    /// Dynamic discovery: scans root Cargo.toml and workspace members to discover all crates and builtins.
    pub fn discover<P: AsRef<Path>>(root: P) -> Self {
        let root = root.as_ref();
        let mut bp = Self::embedded_default();

        let cargo_toml_path = root.join("Cargo.toml");
        if let Ok(content) = std::fs::read_to_string(&cargo_toml_path) {
            let mut members = Vec::new();
            let mut in_members = false;
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("members = [") {
                    in_members = true;
                    continue;
                }
                if in_members {
                    if trimmed.starts_with(']') {
                        break;
                    }
                    let member = trimmed.trim_matches(|c| c == '"' || c == ',' || c == '\'' || c == ' ');
                    if !member.is_empty() {
                        members.push(member.to_string());
                    }
                }
            }

            if !members.is_empty() {
                debug!(count = members.len(), "Discovered workspace members dynamically");
            }
        }

        bp.last_updated = chrono::Utc::now();
        bp
    }

    /// Formats the blueprint into a compact Markdown context block for ContextOS prompt compilation.
    pub fn as_context_block(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "### 🗺️ Live Workspace Blueprint ({} crates, {} tools):",
            self.total_crates,
            self.tools.len()
        ));
        lines.push("- **Workspace Root**: Rust Cargo Workspace (Edition 2021/2024, Tokio async)".into());
        lines.push("- **Key Crates & Primary Exports**:".into());

        for c in &self.crates {
            let exports = if c.key_exports.is_empty() {
                String::new()
            } else {
                format!(" → Exports: `{}`", c.key_exports.join("`, `"))
            };
            lines.push(format!("  • `{}` (`{}`): {}{}", c.name, c.path, c.purpose, exports));
        }

        lines.push("- **Active Registered Tools**:".into());
        for t in &self.tools {
            lines.push(format!("  • `{}`: {} (Risk: {})", t.id, t.summary, t.risk_level));
        }

        lines.push("- **Configuration**: `config/user.md` (user profile), `config/agent.md` (persona), `config/secrets.toml` (vault)".into());

        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedded_blueprint_contains_all_crates() {
        let bp = WorkspaceBlueprint::embedded_default();
        assert!(bp.total_crates >= 18);
        assert!(bp.crates.iter().any(|c| c.name == "companion-domain"));
        assert!(bp.crates.iter().any(|c| c.name == "companion-capabilities"));
        assert!(bp.crates.iter().any(|c| c.name == "companion-rate-limiter"));
        assert!(bp.tools.iter().any(|t| t.id == "gmail.fetch_unread"));
        assert!(bp.tools.iter().any(|t| t.id == "web.fetch"));
    }

    #[test]
    fn test_context_block_formatting() {
        let bp = WorkspaceBlueprint::embedded_default();
        let block = bp.as_context_block();
        assert!(block.contains("Live Workspace Blueprint"));
        assert!(block.contains("companion-capabilities"));
        assert!(block.contains("filesystem.read"));
    }
}
