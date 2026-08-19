use std::sync::Arc;
use tracing::debug;

use companion_domain::{
    CompiledContext, ContextBudget, ContextGrant, ContextId, ContextSources,
    DataSensitivity, MemoryStatus, MemoryTier, Message, RuntimeError,
};
use companion_memory::KnowledgeGraphStore;

use crate::caching::ContextCache;

/// Core context compiler implementing the 8-stage ContextOS compilation pipeline.
pub struct ContextCompiler {
    cache: Arc<ContextCache>,
}

impl ContextCompiler {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(ContextCache::new()),
        }
    }

    pub fn with_cache(cache: Arc<ContextCache>) -> Self {
        Self { cache }
    }

    /// Compile multi-source context into a budgeted, ranked, and cached message payload.
    pub async fn compile(
        &self,
        sources: &ContextSources,
        budget: &ContextBudget,
        grant: Option<&ContextGrant>,
    ) -> Result<CompiledContext, RuntimeError> {
        let mut sections_included = Vec::new();
        let mut included_memory_ids = Vec::new();
        let mut was_truncated = false;

        // 1. Sensitivity Ceiling Check
        let sensitivity_ceiling = grant
            .map(|g| g.sensitivity_ceiling)
            .unwrap_or(DataSensitivity::Internal);

        // 2. Format System Prompt, Persona, User Profile & Policy
        let mut system_blocks = Vec::new();

        // 2a. Agent Persona (if present)
        if let Some(ref persona) = sources.agent_persona_block {
            if !persona.trim().is_empty() {
                system_blocks.push(persona.clone());
                sections_included.push("agent_persona".into());
            }
        }

        // 2b. Base Identity Policy
        let base_identity = sources
            .identity_policy
            .as_deref()
            .unwrap_or("You are Companion, a precise, deterministic AI assistant executing under strict runtime policy.");

        system_blocks.push(base_identity.to_string());
        sections_included.push("identity_policy".into());

        // 2c. User Profile (if present)
        if let Some(ref profile) = sources.user_profile_block {
            if !profile.trim().is_empty() {
                system_blocks.push(profile.clone());
                sections_included.push("user_profile".into());
            }
        }

        // 3. Task Contract Section (if present)
        if let Some(ref contract) = sources.task_contract {
            let contract_text = format!(
                "### Active Task Contract:\n\
                 - Objective: {}\n\
                 - Mode: {:?}\n\
                 - Allowed Tools: {:?}\n\
                 - Max Turns: {}\n\
                 - Max Tool Calls: {}",
                contract.objective,
                contract.mode_profile.primary,
                contract.allowed_tools,
                contract.budget.max_turns,
                contract.budget.max_tool_calls
            );
            system_blocks.push(contract_text);
            sections_included.push("task_contract".into());
        }

        // 3a. Live Workspace Blueprint (if present)
        if let Some(ref blueprint) = sources.workspace_blueprint {
            if !blueprint.trim().is_empty() {
                system_blocks.push(blueprint.clone());
                sections_included.push("workspace_blueprint".into());
            }
        }

        // 3b. Selected Procedural Skills (if present)
        if !sources.selected_skills.is_empty() {
            let mut skill_blocks = Vec::new();
            for skill in &sources.selected_skills {
                let mut step_lines = Vec::new();
                for step in &skill.procedure_graph {
                    step_lines.push(format!("  - Step {}: {} - {}", step.step_id, step.name, step.description));
                }
                skill_blocks.push(format!(
                    "#### Skill: {} (v{}, state={:?}):\n{}\nProcedure:\n{}",
                    skill.name,
                    skill.version,
                    skill.lifecycle_state,
                    skill.description,
                    if step_lines.is_empty() { "  (Direct Execution)".into() } else { step_lines.join("\n") }
                ));
            }
            system_blocks.push(format!("### Selected Procedural Skills:\n{}", skill_blocks.join("\n\n")));
            sections_included.push("selected_skills".into());
        }

        // 4. Working Memory (Scratchpad) (if tier allowed)
        let tier_working_allowed = grant.map(|g| g.is_tier_allowed(MemoryTier::Working)).unwrap_or(true);
        if tier_working_allowed && !sources.working_memory.is_empty() {
            let mut working_lines = Vec::new();
            for note in &sources.working_memory {
                working_lines.push(format!("- {note}"));
            }
            system_blocks.push(format!("### Working Memory (Scratchpad):\n{}", working_lines.join("\n")));
            sections_included.push("working_memory".into());
        }

        // 5. Hierarchical Memories & KG Facts (ranked and filtered by trust/sensitivity/budget)
        let tier_semantic_allowed = grant.map(|g| g.is_tier_allowed(MemoryTier::Semantic)).unwrap_or(true);
        let mut memory_chars_used = 0;
        let memory_char_limit = budget.memory_budget * 4;

        if tier_semantic_allowed && !sources.recalled_memories.is_empty() {
            let mut filtered_memories: Vec<_> = sources
                .recalled_memories
                .iter()
                .filter(|m| m.item.status == MemoryStatus::Active)
                .filter(|m| {
                    if let Some(g) = grant {
                        g.is_tier_allowed(m.tier)
                    } else {
                        true
                    }
                })
                .filter(|m| {
                    // Check sensitivity tag if present in metadata
                    if let Some(sens_str) = m.item.metadata.get("sensitivity").and_then(|v| v.as_str()) {
                        let item_sens = match sens_str.to_lowercase().as_str() {
                            "public" => DataSensitivity::Public,
                            "internal" => DataSensitivity::Internal,
                            "confidential" => DataSensitivity::Confidential,
                            "restricted" => DataSensitivity::Restricted,
                            _ => DataSensitivity::Internal,
                        };
                        item_sens.is_allowed_by(sensitivity_ceiling)
                    } else {
                        true
                    }
                })
                .collect();

            // Rank by score descending
            filtered_memories.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

            let mut mem_lines = Vec::new();
            for m in filtered_memories {
                let line = format!("- [{}] (trust={:?}) {}", m.tier, m.item.trust_class, m.item.content);
                if memory_chars_used + line.len() <= memory_char_limit {
                    memory_chars_used += line.len();
                    included_memory_ids.push(m.item.memory_id);
                    mem_lines.push(line);
                } else {
                    was_truncated = true;
                }
            }

            if !mem_lines.is_empty() {
                system_blocks.push(format!("### Relevant Long-Term Memory:\n{}", mem_lines.join("\n")));
                sections_included.push("hierarchical_memory".into());
            }
        }

        // Knowledge Graph Facts
        let tier_relational_allowed = grant.map(|g| g.is_tier_allowed(MemoryTier::Relational)).unwrap_or(true);
        if tier_relational_allowed && !sources.graph_facts.is_empty() {
            let fact_str = KnowledgeGraphStore::format_facts(&sources.graph_facts);
            if memory_chars_used + fact_str.len() <= memory_char_limit {
                system_blocks.push(format!("### Knowledge Graph Context:\n{fact_str}"));
                sections_included.push("graph_facts".into());
            } else {
                was_truncated = true;
            }
        }

        // 6. Artifact Excerpts and Dependency Outputs (within artifacts_budget)
        let artifacts_char_limit = budget.artifacts_budget * 4;
        let mut artifacts_chars_used = 0;
        let mut artifact_lines = Vec::new();

        for (title, excerpt) in &sources.artifact_excerpts {
            let entry = format!("- **{}**: {}", title, excerpt);
            if artifacts_chars_used + entry.len() <= artifacts_char_limit {
                artifacts_chars_used += entry.len();
                artifact_lines.push(entry);
            } else {
                was_truncated = true;
            }
        }

        for (dep_name, dep_result) in &sources.dependency_outputs {
            let entry = format!("- **Dependency `{}`**: {}", dep_name, dep_result);
            if artifacts_chars_used + entry.len() <= artifacts_char_limit {
                artifacts_chars_used += entry.len();
                artifact_lines.push(entry);
            } else {
                was_truncated = true;
            }
        }

        if !artifact_lines.is_empty() {
            system_blocks.push(format!("### Artifacts & Dependency Context:\n{}", artifact_lines.join("\n")));
            sections_included.push("artifacts_and_dependencies".into());
        }

        // Assemble combined System Message
        let full_system_prompt = system_blocks.join("\n\n");
        let mut compiled_messages = vec![Message::system(full_system_prompt.clone())];

        // 7. Session Conversation History (within history_budget)
        let history_char_limit = budget.history_budget * 4;
        let mut history_chars_used = 0;
        let mut packed_history = Vec::new();

        // Pack from newest backwards to preserve immediate context
        for msg in sources.session_turns.iter().rev() {
            let msg_len = msg.content.len();
            if history_chars_used + msg_len <= history_char_limit {
                history_chars_used += msg_len;
                packed_history.push(msg.clone());
            } else {
                was_truncated = true;
                break;
            }
        }
        packed_history.reverse();
        compiled_messages.extend(packed_history);
        if !sources.session_turns.is_empty() {
            sections_included.push("session_history".into());
        }

        // 8. User Input (if provided separately from session history)
        if let Some(ref user_input) = sources.user_input {
            compiled_messages.push(Message::user(user_input.clone()));
            sections_included.push("user_input".into());
        }

        // Compute Token Estimate (1 token ~ 4 chars)
        let total_chars: usize = compiled_messages
            .iter()
            .map(|m| m.content.len())
            .sum();
        let estimated_tokens = total_chars / 4;

        // Compute Stable Prefix SHA256 Fingerprint for prompt caching
        let prefix_data = format!(
            "{}\n---\n{:?}",
            full_system_prompt,
            sources.selected_tools
        );
        let cache_fingerprint = ContextCache::compute_fingerprint(&prefix_data);
        self.cache.record_access(&cache_fingerprint).await;

        debug!(
            estimated_tokens,
            sections = ?sections_included,
            fingerprint = %cache_fingerprint,
            "compiled context payload"
        );

        Ok(CompiledContext {
            context_id: ContextId::new(),
            messages: compiled_messages,
            estimated_tokens,
            cache_fingerprint,
            included_memory_ids,
            included_artifact_ids: Vec::new(),
            sections_included,
            was_truncated,
        })
    }

    pub fn cache(&self) -> &Arc<ContextCache> {
        &self.cache
    }
}

impl Default for ContextCompiler {
    fn default() -> Self {
        Self::new()
    }
}
