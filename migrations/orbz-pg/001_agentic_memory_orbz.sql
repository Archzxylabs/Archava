-- Archava SOTA Agentic Memory Ledger: Ava Orbz
-- Schema: archava_orbz
-- Implements: OpenViking (URI + L0/L1/L2) + Graphiti (Bi-temporal invalidation) + CoALA 4 Quadrants

CREATE SCHEMA IF NOT EXISTS archava_orbz;

-- 1. Semantic Memory Nodes (Bi-Temporal Knowledge Graph with Progressive L0/L1/L2)
CREATE TABLE IF NOT EXISTS archava_orbz.nodes (
    id UUID PRIMARY KEY,
    tenant_id VARCHAR(64) NOT NULL,
    uri VARCHAR(255) NOT NULL,
    scope VARCHAR(32) NOT NULL, -- 'public', 'department', 'entity', 'sovereign'
    category VARCHAR(64) NOT NULL, -- 'sop', 'catalog', 'preference', 'vendor', 'tax', etc.
    entity_id VARCHAR(64), -- optional linked entity (e.g. cust_123, vend_abc)
    min_authority VARCHAR(16) NOT NULL DEFAULT 'L1', -- 'L1', 'L2', 'L3', 'L4', 'L5'
    
    -- Progressive Disclosure Tiers
    summary_l0 TEXT NOT NULL,      -- ~50 tokens: Quick intent & filter
    overview_l1 TEXT NOT NULL,     -- ~500 tokens: Working context knowledge map
    detail_l2 JSONB,               -- Raw payload / full document, loaded on demand
    
    -- Bi-Temporal Validity (Graphiti Paradigm)
    tx_timestamp TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP, -- When recorded in DB
    valid_from TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,   -- When true in reality
    valid_to TIMESTAMPTZ,                                       -- NULL = active, Set = superseded/expired
    superseded_by UUID REFERENCES archava_orbz.nodes(id),
    
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_orbz_nodes_active 
ON archava_orbz.nodes(tenant_id, scope, category) 
WHERE valid_to IS NULL;

CREATE INDEX IF NOT EXISTS idx_orbz_nodes_uri 
ON archava_orbz.nodes(tenant_id, uri);

CREATE INDEX IF NOT EXISTS idx_orbz_nodes_entity 
ON archava_orbz.nodes(tenant_id, entity_id) 
WHERE entity_id IS NOT NULL;

-- 2. Episodic Memory (Time-Series Interaction & Dialogue Stream)
CREATE TABLE IF NOT EXISTS archava_orbz.episodes (
    id UUID PRIMARY KEY,
    tenant_id VARCHAR(64) NOT NULL,
    session_id VARCHAR(64) NOT NULL,
    user_id VARCHAR(64) NOT NULL,
    user_role VARCHAR(32) NOT NULL,
    channel VARCHAR(32) NOT NULL DEFAULT 'web', -- 'web', 'whatsapp', 'voice', 'api'
    
    user_utterance TEXT NOT NULL,
    agent_response TEXT NOT NULL,
    tools_called JSONB DEFAULT '[]'::jsonb,
    context_tokens_used INT NOT NULL DEFAULT 0,
    
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_orbz_episodes_tenant_session 
ON archava_orbz.episodes(tenant_id, session_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_orbz_episodes_user 
ON archava_orbz.episodes(tenant_id, user_id, created_at DESC);

-- 3. Procedural Memory (Self-Evolving Cockpit Trajectories & Reusable Workflows)
CREATE TABLE IF NOT EXISTS archava_orbz.trajectories (
    id UUID PRIMARY KEY,
    tenant_id VARCHAR(64) NOT NULL,
    mission_code VARCHAR(64) NOT NULL,
    title VARCHAR(128) NOT NULL,
    description TEXT NOT NULL,
    recipe_dag JSONB NOT NULL, -- Serialized execution steps and tool chain
    success_count INT NOT NULL DEFAULT 1,
    failure_count INT NOT NULL DEFAULT 0,
    avg_duration_ms BIGINT NOT NULL DEFAULT 0,
    
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_orbz_trajectories_lookup 
ON archava_orbz.trajectories(tenant_id, mission_code);

-- 4. Enable Row-Level Security (RLS) on all memory tables with mandatory FORCE
ALTER TABLE archava_orbz.nodes ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_orbz.nodes FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_orbz.episodes ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_orbz.episodes FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_orbz.trajectories ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_orbz.trajectories FORCE ROW LEVEL SECURITY;

-- Default tenant isolation policies (Active when RLS is enforced)
-- Supports both 'archava.tenant_id' (core transaction contract) and 'archava.current_tenant'
CREATE POLICY orbz_nodes_tenant_isolation ON archava_orbz.nodes
    FOR ALL
    USING (tenant_id = COALESCE(NULLIF(current_setting('archava.tenant_id', true), ''), NULLIF(current_setting('archava.current_tenant', true), '')));

CREATE POLICY orbz_episodes_tenant_isolation ON archava_orbz.episodes
    FOR ALL
    USING (tenant_id = COALESCE(NULLIF(current_setting('archava.tenant_id', true), ''), NULLIF(current_setting('archava.current_tenant', true), '')));

CREATE POLICY orbz_trajectories_tenant_isolation ON archava_orbz.trajectories
    FOR ALL
    USING (tenant_id = COALESCE(NULLIF(current_setting('archava.tenant_id', true), ''), NULLIF(current_setting('archava.current_tenant', true), '')));
