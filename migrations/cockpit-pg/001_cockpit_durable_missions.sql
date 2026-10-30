-- Archava SOTA Task Orchestration Ledger: Ava Cockpit
-- Schema: archava_cockpit
-- Implements: Temporal.io (Durable Checkpoints) + LangGraph (Cyclic DAG) + OpenHands (EventStream)

CREATE SCHEMA IF NOT EXISTS archava_cockpit;

-- 1. Missions (Top-Level Autonomous Objectives)
CREATE TABLE IF NOT EXISTS archava_cockpit.missions (
    id UUID PRIMARY KEY,
    tenant_id VARCHAR(64) NOT NULL,
    mission_code VARCHAR(64) NOT NULL,
    title VARCHAR(255) NOT NULL,
    objective TEXT NOT NULL,
    
    -- Aviation Flight States
    -- 'pre_flight'      : In planning / DAG formulation
    -- 'in_flight'       : Actively executing background workers
    -- 'holding_pattern' : Paused at L4 Gate (Awaiting human/owner approval)
    -- 'touchdown'       : Successfully completed and verified
    -- 'aborted'         : Rolled back or cancelled
    status VARCHAR(32) NOT NULL DEFAULT 'pre_flight',
    
    initiator_user_id VARCHAR(64) NOT NULL,
    initiator_role VARCHAR(32) NOT NULL,
    required_authority VARCHAR(16) NOT NULL DEFAULT 'L1', -- L1 to L5
    
    holding_reason TEXT,
    holding_signal_key VARCHAR(64), -- Unique resume webhook key
    
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_cockpit_missions_status 
ON archava_cockpit.missions(tenant_id, status);

CREATE INDEX IF NOT EXISTS idx_cockpit_missions_signal 
ON archava_cockpit.missions(holding_signal_key) 
WHERE holding_signal_key IS NOT NULL;

-- 2. Flight Steps (The Directed Acyclic Graph - DAG of Work Items)
CREATE TABLE IF NOT EXISTS archava_cockpit.flight_steps (
    id UUID PRIMARY KEY,
    mission_id UUID NOT NULL REFERENCES archava_cockpit.missions(id) ON DELETE CASCADE,
    tenant_id VARCHAR(64) NOT NULL,
    step_index INT NOT NULL,
    step_name VARCHAR(128) NOT NULL,
    tool_target VARCHAR(128) NOT NULL, -- e.g. 'general_ledger.post_journal'
    
    payload_input JSONB NOT NULL DEFAULT '{}'::jsonb,
    payload_output JSONB,
    
    -- 'pending', 'running', 'passed', 'failed', 'skipped'
    status VARCHAR(32) NOT NULL DEFAULT 'pending',
    
    depends_on_steps INT[] DEFAULT '{}',
    subagent_role VARCHAR(64) NOT NULL DEFAULT 'domain_worker',
    
    -- Claude Code style compressed data summary
    isolated_context_summary TEXT,
    error_message TEXT,
    
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    completed_at TIMESTAMPTZ,
    
    CONSTRAINT uq_mission_step UNIQUE (mission_id, step_index)
);

CREATE INDEX IF NOT EXISTS idx_cockpit_steps_mission 
ON archava_cockpit.flight_steps(mission_id, step_index);

-- 3. Flight Event Stream (OpenHands Style Immutable Event Nervous System)
CREATE TABLE IF NOT EXISTS archava_cockpit.flight_events (
    id BIGSERIAL PRIMARY KEY,
    mission_id UUID NOT NULL REFERENCES archava_cockpit.missions(id) ON DELETE CASCADE,
    tenant_id VARCHAR(64) NOT NULL,
    
    -- 'action_dispatched', 'observation_returned', 'status_changed', 'checkpoint_saved'
    event_type VARCHAR(32) NOT NULL,
    event_payload JSONB NOT NULL,
    
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_cockpit_events_mission 
ON archava_cockpit.flight_events(mission_id, created_at ASC);

-- 4. Durable Execution Checkpoints (Temporal.io Style Instant Crash Recovery)
CREATE TABLE IF NOT EXISTS archava_cockpit.checkpoints (
    mission_id UUID PRIMARY KEY REFERENCES archava_cockpit.missions(id) ON DELETE CASCADE,
    tenant_id VARCHAR(64) NOT NULL,
    current_step_index INT NOT NULL,
    checkpoint_state JSONB NOT NULL, -- Serialized execution context
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- 5. Enable Row-Level Security (RLS) with mandatory FORCE
ALTER TABLE archava_cockpit.missions ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_cockpit.missions FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_cockpit.flight_steps ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_cockpit.flight_steps FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_cockpit.flight_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_cockpit.flight_events FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_cockpit.checkpoints ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_cockpit.checkpoints FORCE ROW LEVEL SECURITY;

-- Supports both 'archava.tenant_id' (core transaction contract) and 'archava.current_tenant'
CREATE POLICY cockpit_missions_tenant_isolation ON archava_cockpit.missions
    FOR ALL
    USING (tenant_id = COALESCE(NULLIF(current_setting('archava.tenant_id', true), ''), NULLIF(current_setting('archava.current_tenant', true), '')));

CREATE POLICY cockpit_steps_tenant_isolation ON archava_cockpit.flight_steps
    FOR ALL
    USING (tenant_id = COALESCE(NULLIF(current_setting('archava.tenant_id', true), ''), NULLIF(current_setting('archava.current_tenant', true), '')));

CREATE POLICY cockpit_events_tenant_isolation ON archava_cockpit.flight_events
    FOR ALL
    USING (tenant_id = COALESCE(NULLIF(current_setting('archava.tenant_id', true), ''), NULLIF(current_setting('archava.current_tenant', true), '')));

CREATE POLICY cockpit_checkpoints_tenant_isolation ON archava_cockpit.checkpoints
    FOR ALL
    USING (tenant_id = COALESCE(NULLIF(current_setting('archava.tenant_id', true), ''), NULLIF(current_setting('archava.current_tenant', true), '')));
