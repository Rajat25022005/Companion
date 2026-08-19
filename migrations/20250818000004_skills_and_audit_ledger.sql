-- Companion Enterprise Phase 4 Schema
-- Skills (SkillOS Versioned Registry)
-- Audit Ledger (Cryptographic SHA256 Hash Chain)

CREATE TABLE IF NOT EXISTS skills (
    skill_id         UUID PRIMARY KEY,
    name             TEXT NOT NULL,
    version          INTEGER NOT NULL,
    description      TEXT NOT NULL,
    lifecycle_state  TEXT NOT NULL,
    schema_json      JSONB NOT NULL DEFAULT '{}',
    metrics          JSONB NOT NULL DEFAULT '{}',
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_skill_name_version UNIQUE (name, version)
);

CREATE INDEX IF NOT EXISTS idx_skills_name ON skills(name);
CREATE INDEX IF NOT EXISTS idx_skills_state ON skills(lifecycle_state);

CREATE TABLE IF NOT EXISTS audit_ledger (
    sequence         BIGINT PRIMARY KEY,
    timestamp        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    tenant_id        UUID NOT NULL,
    task_id          UUID,
    actor            TEXT NOT NULL,
    action           TEXT NOT NULL,
    details          JSONB NOT NULL DEFAULT '{}',
    prev_hash        TEXT NOT NULL,
    entry_hash       TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_audit_tenant ON audit_ledger(tenant_id);
CREATE INDEX IF NOT EXISTS idx_audit_task ON audit_ledger(task_id);
CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_ledger(timestamp);
