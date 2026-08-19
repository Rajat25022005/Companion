use std::collections::{HashMap, HashSet, VecDeque};
use chrono::Utc;
use tokio::sync::RwLock;
use companion_domain::{Entity, RelationshipTriple};

/// In-memory knowledge graph store for entity-relationship triples.
pub struct KnowledgeGraphStore {
    triples: RwLock<Vec<RelationshipTriple>>,
    entities: RwLock<HashMap<String, Entity>>,
    /// Adjacency: subject -> list of stored triples
    forward_adj: RwLock<HashMap<String, Vec<RelationshipTriple>>>,
    /// Reverse adjacency: object -> list of stored triples
    reverse_adj: RwLock<HashMap<String, Vec<RelationshipTriple>>>,
}

impl KnowledgeGraphStore {
    pub fn new() -> Self {
        Self {
            triples: RwLock::new(Vec::new()),
            entities: RwLock::new(HashMap::new()),
            forward_adj: RwLock::new(HashMap::new()),
            reverse_adj: RwLock::new(HashMap::new()),
        }
    }

    /// Add an entity to the graph.
    pub async fn add_entity(&self, entity: Entity) {
        let key = entity.name.to_lowercase();
        let mut map = self.entities.write().await;
        map.insert(key, entity);
    }

    /// Get an entity by name.
    pub async fn get_entity(&self, name: &str) -> Option<Entity> {
        let key = name.to_lowercase();
        let map = self.entities.read().await;
        map.get(&key).cloned()
    }

    /// Add a relationship triple to the knowledge graph.
    pub async fn add_triple(&self, mut triple: RelationshipTriple) {
        // Normalize subject and object to lowercase for query matching
        triple.subject = triple.subject.to_lowercase();
        triple.object = triple.object.to_lowercase();

        let subj = triple.subject.clone();
        let obj = triple.object.clone();

        {
            let mut fwd = self.forward_adj.write().await;
            fwd.entry(subj).or_default().push(triple.clone());
        }

        {
            let mut rev = self.reverse_adj.write().await;
            rev.entry(obj).or_default().push(triple.clone());
        }

        let mut list = self.triples.write().await;
        list.push(triple);
    }

    /// Perform a multi-hop BFS traversal from `start_entity` up to `max_hops`.
    pub async fn traverse(&self, start_entity: &str, max_hops: u32) -> Vec<RelationshipTriple> {
        let now = Utc::now();
        let start = start_entity.to_lowercase();
        let mut visited_entities = HashSet::new();
        let mut collected_triples = Vec::new();
        let mut queue = VecDeque::new();

        queue.push_back((start.clone(), 0));
        visited_entities.insert(start);

        let fwd = self.forward_adj.read().await;

        while let Some((curr_entity, depth)) = queue.pop_front() {
            if depth >= max_hops {
                continue;
            }

            if let Some(neighbors) = fwd.get(&curr_entity) {
                for triple in neighbors {
                    if triple.is_valid_at(now) {
                        collected_triples.push(triple.clone());

                        let next_obj = &triple.object;
                        if !visited_entities.contains(next_obj) {
                            visited_entities.insert(next_obj.clone());
                            queue.push_back((next_obj.clone(), depth + 1));
                        }
                    }
                }
            }
        }

        collected_triples
    }

    /// Find all direct facts (triples) concerning an entity.
    pub async fn get_entity_facts(&self, entity: &str) -> Vec<RelationshipTriple> {
        self.traverse(entity, 1).await
    }

    /// Format knowledge graph triples as clean Markdown bullet points for prompt injection.
    pub fn format_facts(triples: &[RelationshipTriple]) -> String {
        if triples.is_empty() {
            return String::new();
        }

        let mut lines = Vec::new();
        for t in triples {
            lines.push(format!("- **{}** {} **{}**", t.subject, t.predicate, t.object));
        }
        lines.join("\n")
    }

    /// Count total triples.
    pub async fn triple_count(&self) -> usize {
        let list = self.triples.read().await;
        list.len()
    }
}

impl Default for KnowledgeGraphStore {
    fn default() -> Self {
        Self::new()
    }
}
