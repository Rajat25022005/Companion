-- Companion v2 Phase 1 Schema
-- Task events (event sourcing)
-- Tasks (materialized state)
-- Checkpoints (crash recovery)

CREATE TABLE IF NOT EXISTS task_events (
    event_id       UUID PRIMARY KEY,
    task_id        UUID NOT NULL,
    correlation_id UUID NOT NULL,
    timestamp      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    sequence       BIGINT NOT NULL,
    event_type     TEXT NOT NULL,
    payload        JSONB NOT NULL DEFAULT '{}',
    UNIQUE(task_id, sequence)
);

CREATE INDEX IF NOT EXISTS idx_events_task_seq ON task_events(task_id, sequence);
CREATE INDEX IF NOT EXISTS idx_events_type ON task_events(event_type);
CREATE INDEX IF NOT EXISTS idx_events_timestamp ON task_events(timestamp);

CREATE TABLE IF NOT EXISTS tasks (
    task_id        UUID PRIMARY KEY,
    tenant_id      UUID NOT NULL,
    workspace_id   UUID NOT NULL,
    state          JSONB NOT NULL,
    contract       JSONB NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_tasks_tenant ON tasks(tenant_id);
CREATE INDEX IF NOT EXISTS idx_tasks_state ON tasks USING GIN (state);

CREATE TABLE IF NOT EXISTS checkpoints (
    checkpoint_id  UUID PRIMARY KEY,
    task_id        UUID NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
    sequence       BIGINT NOT NULL,
    state_snapshot JSONB NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_checkpoints_task ON checkpoints(task_id, sequence);
