-- Companion v2 Phase 3 Schema
-- Memories (Episodic, Semantic, Working)
-- Entities & Relationships (Knowledge Graph Triples)

CREATE TABLE IF NOT EXISTS memories (
    memory_id    UUID PRIMARY KEY,
    tenant_id    UUID,
    tier         TEXT NOT NULL,
    content      TEXT NOT NULL,
    metadata     JSONB NOT NULL DEFAULT '{}',
    embedding    REAL[],
    importance   REAL NOT NULL DEFAULT 1.0,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    accessed_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    access_count INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_memories_tier ON memories(tier);
CREATE INDEX IF NOT EXISTS idx_memories_tenant ON memories(tenant_id);
CREATE INDEX IF NOT EXISTS idx_memories_accessed ON memories(accessed_at);

CREATE TABLE IF NOT EXISTS entities (
    entity_id   UUID PRIMARY KEY,
    tenant_id   UUID,
    name        TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    attributes  JSONB NOT NULL DEFAULT '{}',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_entities_name ON entities(name);
CREATE INDEX IF NOT EXISTS idx_entities_type ON entities(entity_type);

CREATE TABLE IF NOT EXISTS relationships (
    relationship_id UUID PRIMARY KEY,
    subject         TEXT NOT NULL,
    predicate       TEXT NOT NULL,
    object          TEXT NOT NULL,
    weight          REAL NOT NULL DEFAULT 1.0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_rel_subject ON relationships(subject);
CREATE INDEX IF NOT EXISTS idx_rel_object ON relationships(object);
CREATE INDEX IF NOT EXISTS idx_rel_predicate ON relationships(predicate);
