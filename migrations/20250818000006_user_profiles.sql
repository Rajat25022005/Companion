-- Migration 006: User Profiles and Agent Personas

CREATE TABLE IF NOT EXISTS user_profiles (
    user_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    display_name TEXT NOT NULL,
    handle TEXT,
    timezone TEXT DEFAULT 'UTC',
    profile_markdown TEXT NOT NULL DEFAULT '',
    preferences JSONB NOT NULL DEFAULT '[]',
    current_projects JSONB NOT NULL DEFAULT '[]',
    notes TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS agent_personas (
    persona_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL DEFAULT 'Companion',
    role TEXT NOT NULL DEFAULT 'Autonomous AI Agent',
    persona_markdown TEXT NOT NULL DEFAULT '',
    traits JSONB NOT NULL DEFAULT '[]',
    behavioral_rules JSONB NOT NULL DEFAULT '[]',
    tone JSONB NOT NULL DEFAULT '[]',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
