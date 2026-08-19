-- Companion v2 Phase 2 Schema
-- Goals (long-lived objectives & milestones)
-- Workflows (DAG definitions & execution state)
-- Workflow Checkpoints (step-level durability & replay)
-- CAP Messages (agent-to-agent protocol logs)

CREATE TABLE IF NOT EXISTS goals (
    goal_id            UUID PRIMARY KEY,
    tenant_id          UUID NOT NULL,
    title              TEXT NOT NULL,
    description        TEXT NOT NULL,
    status             TEXT NOT NULL,
    milestones         JSONB NOT NULL DEFAULT '[]',
    active_workflow_id UUID,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_goals_tenant ON goals(tenant_id);
CREATE INDEX IF NOT EXISTS idx_goals_status ON goals(status);

CREATE TABLE IF NOT EXISTS workflows (
    workflow_id   UUID PRIMARY KEY,
    goal_id       UUID REFERENCES goals(goal_id) ON DELETE SET NULL,
    name          TEXT NOT NULL,
    status        TEXT NOT NULL,
    definition    JSONB NOT NULL,
    state         JSONB NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_workflows_goal ON workflows(goal_id);
CREATE INDEX IF NOT EXISTS idx_workflows_status ON workflows(status);

CREATE TABLE IF NOT EXISTS workflow_checkpoints (
    checkpoint_id  UUID PRIMARY KEY,
    workflow_id    UUID NOT NULL REFERENCES workflows(workflow_id) ON DELETE CASCADE,
    sequence       BIGINT NOT NULL,
    state_snapshot JSONB NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(workflow_id, sequence)
);

CREATE INDEX IF NOT EXISTS idx_wf_checkpoints_seq ON workflow_checkpoints(workflow_id, sequence);

CREATE TABLE IF NOT EXISTS cap_messages (
    message_id      UUID PRIMARY KEY,
    correlation_id  UUID NOT NULL,
    conversation_id UUID NOT NULL,
    sender          JSONB NOT NULL,
    recipient       JSONB NOT NULL,
    pattern         JSONB NOT NULL,
    payload         JSONB NOT NULL,
    references_data JSONB NOT NULL DEFAULT '[]',
    timestamp       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_cap_correlation ON cap_messages(correlation_id);
CREATE INDEX IF NOT EXISTS idx_cap_conversation ON cap_messages(conversation_id);
CREATE INDEX IF NOT EXISTS idx_cap_timestamp ON cap_messages(timestamp);
