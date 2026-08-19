use std::sync::Arc;
use tracing::debug;

use companion_domain::{
    ConsolidationReport, MemoryFilter, MemoryItem, MemorySearchResult, MemoryTier,
    RelationshipTriple, RuntimeError,
};
use companion_events::TaskEvent;

use crate::consolidation::MemoryConsolidator;
use crate::embeddings::EmbeddingProvider;
use crate::episodic::EpisodicRecorder;
use crate::graph_store::KnowledgeGraphStore;
use crate::session_store::SessionStore;
use crate::vector_store::VectorStore;
use crate::working::WorkingMemory;

/// Unified memory manager coordinating all 7 hierarchical memory tiers and consolidation.
pub struct MemoryManager {
    embedder: Arc<dyn EmbeddingProvider>,
    working: Arc<WorkingMemory>,
    session_store: Arc<SessionStore>,
    vector_store: Arc<VectorStore>,
    graph_store: Arc<KnowledgeGraphStore>,
    episodic: Arc<EpisodicRecorder>,
    consolidator: Arc<MemoryConsolidator>,
}

impl MemoryManager {
    pub fn new(embedder: Arc<dyn EmbeddingProvider>) -> Self {
        let working = Arc::new(WorkingMemory::new());
        let session_store = Arc::new(SessionStore::new());
        let vector_store = Arc::new(VectorStore::new());
        let graph_store = Arc::new(KnowledgeGraphStore::new());
        let episodic = Arc::new(EpisodicRecorder::new(
            embedder.clone(),
            vector_store.clone(),
        ));
        let consolidator = Arc::new(MemoryConsolidator::new(
            embedder.clone(),
            vector_store.clone(),
            graph_store.clone(),
        ));

        Self {
            embedder,
            working,
            session_store,
            vector_store,
            graph_store,
            episodic,
            consolidator,
        }
    }

    /// Store a piece of text knowledge into semantic memory with embedding vector (legacy helper).
    pub async fn remember(
        &self,
        content: &str,
        tier: MemoryTier,
        importance: f32,
    ) -> Result<MemoryItem, RuntimeError> {
        let embedding = self.embedder.embed(content).await?;
        let item = MemoryItem::new(tier, content)
            .with_embedding(embedding)
            .with_importance(importance);

        self.vector_store.insert(item.clone()).await;
        debug!(memory_id = %item.memory_id, tier = %tier, "stored memory item");
        Ok(item)
    }

    /// Store a full memory record with custom trust class, subject, and provenance.
    pub async fn remember_record(&self, mut record: MemoryItem) -> Result<MemoryItem, RuntimeError> {
        if record.embedding.is_none() {
            let embedding = self.embedder.embed(&record.content).await?;
            record = record.with_embedding(embedding);
        }

        self.vector_store.insert(record.clone()).await;
        debug!(memory_id = %record.memory_id, tier = %record.tier, trust = ?record.trust_class, "stored memory record");
        Ok(record)
    }

    /// Add a knowledge graph fact (subject -[predicate]-> object).
    pub async fn add_fact(&self, subject: &str, predicate: &str, object: &str) {
        let triple = RelationshipTriple::new(subject, predicate, object);
        self.graph_store.add_triple(triple).await;
    }

    /// Query the knowledge graph starting from `entity` up to `hops`.
    pub async fn query_graph(&self, entity: &str, hops: u32) -> Vec<RelationshipTriple> {
        self.graph_store.traverse(entity, hops).await
    }

    /// Recall relevant memories across vector store using semantic search (legacy signature).
    pub async fn recall(
        &self,
        query: &str,
        limit: usize,
        min_similarity: f32,
    ) -> Result<Vec<MemorySearchResult>, RuntimeError> {
        let query_vec = self.embedder.embed(query).await?;
        let results = self
            .vector_store
            .search(&query_vec, limit, min_similarity, None)
            .await;

        Ok(results)
    }

    /// Multi-factor ranked recall with structured filter.
    pub async fn recall_ranked(
        &self,
        query: &str,
        limit: usize,
        min_similarity: f32,
        filter: &MemoryFilter,
    ) -> Result<Vec<MemorySearchResult>, RuntimeError> {
        let query_vec = self.embedder.embed(query).await?;
        let results = self
            .vector_store
            .search_with_filter(&query_vec, limit, min_similarity, filter)
            .await;

        Ok(results)
    }

    /// Run the offline Dream Cycle / Memory Consolidation engine over episodic memories and events.
    pub async fn consolidate(
        &self,
        episodes: &[MemoryItem],
        events: &[TaskEvent],
    ) -> Result<ConsolidationReport, RuntimeError> {
        self.consolidator.consolidate(episodes, events).await
    }

    /// Assemble multi-tier contextual memories into a formatted Markdown block for prompt injection.
    pub async fn assemble_context(&self, query: &str, token_budget: usize) -> Result<String, RuntimeError> {
        let mut sections = Vec::new();

        // 1. Semantic and Episodic Recall
        let memories = self.recall(query, 5, 0.1).await?;
        if !memories.is_empty() {
            let mut mem_lines = Vec::new();
            for m in &memories {
                mem_lines.push(format!("- [{}] {}", m.tier, m.item.content));
            }
            sections.push(format!("### Relevant Memories:\n{}", mem_lines.join("\n")));
        }

        // 2. Knowledge Graph Entity Traversals
        let query_words: Vec<&str> = query.split_whitespace().collect();
        let mut graph_facts = Vec::new();
        for word in query_words {
            let clean = word.trim_matches(|c: char| !c.is_alphanumeric());
            if clean.len() > 2 {
                let facts = self.graph_store.get_entity_facts(clean).await;
                graph_facts.extend(facts);
            }
        }

        if !graph_facts.is_empty() {
            graph_facts.dedup_by(|a, b| {
                a.subject == b.subject && a.predicate == b.predicate && a.object == b.object
            });
            let fact_str = KnowledgeGraphStore::format_facts(&graph_facts);
            sections.push(format!("### Knowledge Graph Context:\n{fact_str}"));
        }

        let combined = sections.join("\n\n");

        // Token budget estimation and truncation
        let max_chars = token_budget * 4;
        if combined.len() > max_chars {
            let truncated = &combined[..max_chars];
            Ok(format!("{truncated}...\n(Truncated for context budget)"))
        } else {
            Ok(combined)
        }
    }

    pub fn working_memory(&self) -> &Arc<WorkingMemory> {
        &self.working
    }

    pub fn session_store(&self) -> &Arc<SessionStore> {
        &self.session_store
    }

    pub fn episodic_recorder(&self) -> &Arc<EpisodicRecorder> {
        &self.episodic
    }

    pub fn graph_store(&self) -> &Arc<KnowledgeGraphStore> {
        &self.graph_store
    }

    pub fn vector_store(&self) -> &Arc<VectorStore> {
        &self.vector_store
    }

    pub fn consolidator(&self) -> &Arc<MemoryConsolidator> {
        &self.consolidator
    }

    pub fn embedder(&self) -> &Arc<dyn EmbeddingProvider> {
        &self.embedder
    }
}
