use std::sync::Arc;
use chrono::Utc;
use tracing::{debug, info};
use companion_domain::{
    ConsolidationReport, MemoryItem, MemoryStatus, MemoryTier,
    RelationshipTriple, RuntimeError, TrustClass,
};
use companion_events::{TaskEvent, TaskEventType};

use crate::embeddings::EmbeddingProvider;
use crate::graph_store::KnowledgeGraphStore;
use crate::vector_store::VectorStore;

/// The Memory Consolidator / Dream Cycle plane.
///
/// Runs asynchronously / offline to consume recent task execution episodes and events,
/// extract long-term semantic knowledge and knowledge graph triples, resolve contradictions,
/// and supersede obsolete memory records.
pub struct MemoryConsolidator {
    embedder: Arc<dyn EmbeddingProvider>,
    vector_store: Arc<VectorStore>,
    graph_store: Arc<KnowledgeGraphStore>,
}

impl MemoryConsolidator {
    pub fn new(
        embedder: Arc<dyn EmbeddingProvider>,
        vector_store: Arc<VectorStore>,
        graph_store: Arc<KnowledgeGraphStore>,
    ) -> Self {
        Self {
            embedder,
            vector_store,
            graph_store,
        }
    }

    /// Run a consolidation cycle over the provided batch of episodic memory items and task events.
    pub async fn consolidate(
        &self,
        episodes: &[MemoryItem],
        events: &[TaskEvent],
    ) -> Result<ConsolidationReport, RuntimeError> {
        let start_time = Utc::now();
        let mut facts_extracted = 0;
        let mut triples_created = 0;
        let mut contradictions_resolved = 0;
        let mut records_superseded = 0;

        debug!(
            episodes_count = episodes.len(),
            events_count = events.len(),
            "starting memory consolidation (dream cycle)"
        );

        // 1. Process Task Events for deterministic tool-verified state changes
        for event in events {
            match event.event_type {
                TaskEventType::ToolCallCompleted => {
                    if let Some(tool_name) = event.payload.get("name").and_then(|v| v.as_str()) {
                        let result_preview = event
                            .payload
                            .get("result")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        // Extract tool-verified facts
                        let fact_content = format!("Tool `{tool_name}` successfully executed: {result_preview}");
                        let embedding = self.embedder.embed(&fact_content).await?;

                        let item = MemoryItem::new(MemoryTier::Semantic, fact_content)
                            .with_embedding(embedding)
                            .with_trust_class(TrustClass::ToolVerified)
                            .with_importance(1.1)
                            .with_metadata(serde_json::json!({
                                "source": "tool_execution",
                                "tool": tool_name,
                                "event_id": event.event_id.to_string(),
                            }));

                        self.vector_store.insert(item).await;
                        facts_extracted += 1;

                        // Create relationship triple if target entity identifiable
                        if let Some(target) = event.payload.get("target").and_then(|v| v.as_str()) {
                            let triple = RelationshipTriple::new(tool_name, "produced", target)
                                .with_weight(1.0)
                                .with_confidence(0.95);
                            self.graph_store.add_triple(triple).await;
                            triples_created += 1;
                        }
                    }
                }
                TaskEventType::TaskCompleted => {
                    if let Some(objective) = event.payload.get("objective").and_then(|v| v.as_str()) {
                        let fact_content = format!("Successfully accomplished goal: {objective}");
                        let embedding = self.embedder.embed(&fact_content).await?;

                        let item = MemoryItem::new(MemoryTier::Semantic, fact_content)
                            .with_embedding(embedding)
                            .with_trust_class(TrustClass::ToolVerified)
                            .with_importance(1.3);

                        self.vector_store.insert(item).await;
                        facts_extracted += 1;
                    }
                }
                _ => {}
            }
        }

        // 2. Process Episodic Memories for recurring procedures and semantic extraction
        for episode in episodes {
            if episode.tier == MemoryTier::Episodic {
                // Extract structured insights from episode content lines
                let lines: Vec<&str> = episode.content.lines().collect();
                for line in lines {
                    let trimmed = line.trim();
                    if trimmed.starts_with("Task:") || trimmed.starts_with("Objective:") {
                        let content = format!("Learned experience: {trimmed}");
                        let embedding = self.embedder.embed(&content).await?;

                        let item = MemoryItem::new(MemoryTier::Semantic, content)
                            .with_embedding(embedding)
                            .with_trust_class(episode.trust_class)
                            .with_importance(1.0);

                        self.vector_store.insert(item).await;
                        facts_extracted += 1;
                    }
                }
            }
        }

        // 3. Contradiction Resolution & Superseding
        // Check active memories for conflicting statements and supersede lower authority or older records
        let all_memories = self.vector_store.list_all().await;
        for i in 0..all_memories.len() {
            for j in (i + 1)..all_memories.len() {
                let mem_a = &all_memories[i];
                let mem_b = &all_memories[j];

                if mem_a.status != MemoryStatus::Active || mem_b.status != MemoryStatus::Active {
                    continue;
                }

                // If both share the same explicit subject but have different contents
                if let (Some(subj_a), Some(subj_b)) = (&mem_a.subject, &mem_b.subject) {
                    if subj_a.eq_ignore_ascii_case(subj_b) && mem_a.content != mem_b.content {
                        // Check authority rank
                        if mem_a.trust_class.authority_rank() > mem_b.trust_class.authority_rank() {
                            self.vector_store.supersede(&mem_b.memory_id, mem_a.memory_id).await;
                            contradictions_resolved += 1;
                            records_superseded += 1;
                        } else if mem_b.trust_class.authority_rank() > mem_a.trust_class.authority_rank() {
                            self.vector_store.supersede(&mem_a.memory_id, mem_b.memory_id).await;
                            contradictions_resolved += 1;
                            records_superseded += 1;
                        } else if mem_b.created_at > mem_a.created_at {
                            // Newer item with equal trust supersedes older
                            self.vector_store.supersede(&mem_a.memory_id, mem_b.memory_id).await;
                            contradictions_resolved += 1;
                            records_superseded += 1;
                        }
                    }
                }
            }
        }

        let end_time = Utc::now();
        let duration_ms = (end_time - start_time).num_milliseconds().max(0) as u64;

        let report = ConsolidationReport {
            timestamp: end_time,
            duration_ms,
            episodes_processed: episodes.len(),
            facts_extracted,
            triples_created,
            contradictions_resolved,
            records_superseded,
            skills_synthesized: 0,
        };

        info!(
            duration_ms = report.duration_ms,
            facts = report.facts_extracted,
            triples = report.triples_created,
            superseded = report.records_superseded,
            "memory consolidation cycle complete"
        );

        Ok(report)
    }
}
