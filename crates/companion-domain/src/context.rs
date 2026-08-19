use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::capability::ToolDefinition;
use crate::ids::{AgentId, ArtifactId, ContextId, GrantId, MemoryId, TaskId};
use crate::memory::{MemorySearchResult, MemoryTier, RelationshipTriple};
use crate::model::Message;
use crate::task::TaskContract;

// ---------------------------------------------------------------------------
// Data Sensitivity & Classification
// ---------------------------------------------------------------------------

/// Data sensitivity classification for least-privilege context compilation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DataSensitivity {
    /// Public information that can be shared without restrictions.
    Public,
    /// Standard internal project and workspace content.
    #[default]
    Internal,
    /// Confidential data requiring specific principal/agent authorization.
    Confidential,
    /// Highly restricted data requiring explicit policy approval; no implicit model routing.
    Restricted,
}

impl DataSensitivity {
    pub fn rank(&self) -> u8 {
        match self {
            Self::Public => 0,
            Self::Internal => 1,
            Self::Confidential => 2,
            Self::Restricted => 3,
        }
    }

    /// Check if this sensitivity level is allowed given an authorized ceiling.
    pub fn is_allowed_by(&self, ceiling: DataSensitivity) -> bool {
        self.rank() <= ceiling.rank()
    }
}

// ---------------------------------------------------------------------------
// Context Budgeting
// ---------------------------------------------------------------------------

/// Token allocation quotas across distinct context sections.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextBudget {
    /// Maximum allowed tokens for the entire compiled context.
    pub max_total_tokens: usize,
    /// Budget reserved for system instructions and security policy.
    pub system_budget: usize,
    /// Budget for task contract specifications and constraints.
    pub contract_budget: usize,
    /// Budget for tool / capability definitions.
    pub tools_budget: usize,
    /// Budget for hierarchical memory and knowledge graph facts.
    pub memory_budget: usize,
    /// Budget for session conversation turn history.
    pub history_budget: usize,
    /// Budget for external artifact excerpts and dependency signals.
    pub artifacts_budget: usize,
}

impl ContextBudget {
    /// Create standard balanced budget for a specified total token limit (e.g. 4096).
    pub fn for_total_tokens(total: usize) -> Self {
        Self {
            max_total_tokens: total,
            system_budget: (total * 15) / 100,      // ~15%
            contract_budget: (total * 10) / 100,    // ~10%
            tools_budget: (total * 20) / 100,       // ~20%
            memory_budget: (total * 20) / 100,      // ~20%
            history_budget: (total * 25) / 100,     // ~25%
            artifacts_budget: (total * 10) / 100,   // ~10%
        }
    }
}

impl Default for ContextBudget {
    fn default() -> Self {
        Self::for_total_tokens(4096)
    }
}

// ---------------------------------------------------------------------------
// Context Grants & Requests (Least-Privilege Scoping)
// ---------------------------------------------------------------------------

/// A least-privilege permission grant issued by the Context Broker to an agent or task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextGrant {
    pub grant_id: GrantId,
    pub task_id: TaskId,
    pub agent_id: Option<AgentId>,
    pub allowed_tiers: Vec<MemoryTier>,
    pub sensitivity_ceiling: DataSensitivity,
    pub allowed_fields: Vec<String>,
    pub token_budget: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
}

impl ContextGrant {
    pub fn new(task_id: TaskId, sensitivity_ceiling: DataSensitivity, token_budget: usize) -> Self {
        Self {
            grant_id: GrantId::new(),
            task_id,
            agent_id: None,
            allowed_tiers: vec![
                MemoryTier::Working,
                MemoryTier::Session,
                MemoryTier::Episodic,
                MemoryTier::Semantic,
                MemoryTier::Relational,
            ],
            sensitivity_ceiling,
            allowed_fields: Vec::new(),
            token_budget,
            expires_at: None,
        }
    }

    pub fn with_agent(mut self, agent_id: AgentId) -> Self {
        self.agent_id = Some(agent_id);
        self
    }

    pub fn with_allowed_tiers(mut self, tiers: Vec<MemoryTier>) -> Self {
        self.allowed_tiers = tiers;
        self
    }

    pub fn is_tier_allowed(&self, tier: MemoryTier) -> bool {
        self.allowed_tiers.contains(&tier)
    }

    pub fn is_sensitivity_allowed(&self, sensitivity: DataSensitivity) -> bool {
        sensitivity.is_allowed_by(self.sensitivity_ceiling)
    }
}

/// Request for context sent by an agent or task component.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextRequest {
    pub task_id: TaskId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<AgentId>,
    pub query: String,
    pub max_tokens: Option<usize>,
    pub requested_tiers: Vec<MemoryTier>,
}

// ---------------------------------------------------------------------------
// Context Sources Bundle
// ---------------------------------------------------------------------------

/// The multi-domain input sources provided to the Context Compiler.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContextSources {
    pub identity_policy: Option<String>,
    pub user_profile_block: Option<String>,
    pub agent_persona_block: Option<String>,
    pub task_contract: Option<TaskContract>,
    pub goal_state: Option<String>,
    pub working_memory: Vec<String>,
    pub session_turns: Vec<Message>,
    pub recalled_memories: Vec<MemorySearchResult>,
    pub graph_facts: Vec<RelationshipTriple>,
    pub selected_tools: Vec<ToolDefinition>,
    pub selected_skills: Vec<crate::skill::Skill>,
    pub artifact_excerpts: Vec<(String, String)>,
    pub dependency_outputs: Vec<(String, String)>,
    pub workspace_blueprint: Option<String>,
    pub user_input: Option<String>,
}

// ---------------------------------------------------------------------------
// Compiled Context Output
// ---------------------------------------------------------------------------

/// The final compiled prompt and messages payload ready for model dispatch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledContext {
    pub context_id: ContextId,
    pub messages: Vec<Message>,
    pub estimated_tokens: usize,
    pub cache_fingerprint: String,
    pub included_memory_ids: Vec<MemoryId>,
    pub included_artifact_ids: Vec<ArtifactId>,
    pub sections_included: Vec<String>,
    pub was_truncated: bool,
}
