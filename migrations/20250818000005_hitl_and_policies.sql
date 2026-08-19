-- ---------------------------------------------------------------------------
-- Migration 005: HITL Approvals, Declarative Policies & Self-Healing Telemetry
-- ---------------------------------------------------------------------------

-- Human-In-The-Loop Approval Requests
CREATE TABLE IF NOT EXISTS approval_requests (
    approval_id UUID PRIMARY KEY,
    task_id UUID NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
    tenant_id UUID NOT NULL,
    risk_level VARCHAR(32) NOT NULL,
    action_description TEXT NOT NULL,
    requested_capabilities JSONB NOT NULL DEFAULT '[]'::jsonb,
    status VARCHAR(32) NOT NULL DEFAULT 'pending',
    approver VARCHAR(256),
    denial_reason TEXT,
    requested_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    resolved_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_approval_requests_tenant ON approval_requests(tenant_id);
CREATE INDEX IF NOT EXISTS idx_approval_requests_status ON approval_requests(status);
CREATE INDEX IF NOT EXISTS idx_approval_requests_task ON approval_requests(task_id);

-- Declarative Enterprise Policy Rules
CREATE TABLE IF NOT EXISTS policy_rules (
    rule_id UUID PRIMARY KEY,
    name VARCHAR(256) NOT NULL,
    description TEXT NOT NULL,
    condition_json JSONB NOT NULL,
    effect_json JSONB NOT NULL,
    priority INT NOT NULL DEFAULT 100,
    active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_policy_rules_priority ON policy_rules(priority DESC);
CREATE INDEX IF NOT EXISTS idx_policy_rules_active ON policy_rules(active);

-- Autonomous Self-Healing Diagnostic Ledger
CREATE TABLE IF NOT EXISTS self_healing_logs (
    log_id UUID PRIMARY KEY,
    task_id UUID NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
    attempt INT NOT NULL,
    root_cause_category VARCHAR(64) NOT NULL,
    diagnosis_json JSONB NOT NULL,
    compensation_json JSONB,
    outcome VARCHAR(32) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_self_healing_task ON self_healing_logs(task_id);
