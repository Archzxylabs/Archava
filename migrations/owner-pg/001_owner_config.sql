-- The owner-configuration ledger.
--
-- Rust-owned, additively namespaced: the owner surface's users and grants are
-- SEPARATE from the CRM/commerce memberships (owner PRD §6: "tenant-owner
-- configuration memiliki pengguna serta otoritas berbeda"). No TS-owned table
-- is referenced, so this schema can be migrated by the Rust serving stack
-- without changing another lane's migrator.
--
-- Invariants the host store relies on:
--   * one configuration row per (tenant, revision), with revision 1 derived
--     from the catalogue's defaults on first apply (revision 0 is synthetic);
--   * one durable receipt per (tenant, request id) — a duplicate submission
--     replays it, a different plan under the same id is a conflict;
--   * history is append-only and every revision has exactly one audit row;
--   * tenant isolation is forced RLS keyed by archava.tenant_id, the same
--     session-local setting the other ledgers use.

CREATE TABLE archava_owner.owner_grants (
  tenant_id text NOT NULL,
  principal_id text NOT NULL,
  permissions text[] NOT NULL CHECK (permissions <@ ARRAY['config.read','config.apply']::text[]),
  active boolean NOT NULL DEFAULT true,
  PRIMARY KEY (tenant_id, principal_id)
);

CREATE TABLE archava_owner.config_revisions (
  tenant_id text NOT NULL,
  revision bigint NOT NULL CHECK (revision > 0),
  config jsonb NOT NULL CHECK (jsonb_typeof(config) = 'object'),
  base_revision bigint NOT NULL CHECK (base_revision >= 0),
  actor text NOT NULL,
  request_id text NOT NULL,
  plan_hash text NOT NULL,
  catalogue_version text NOT NULL,
  pricing_version text NOT NULL,
  outcome text NOT NULL CHECK (outcome IN ('apply','restore')),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY (tenant_id, revision)
);

CREATE TABLE archava_owner.apply_receipts (
  tenant_id text NOT NULL,
  request_id text NOT NULL,
  revision bigint NOT NULL,
  plan_hash text NOT NULL,
  config jsonb NOT NULL,
  catalogue_version text NOT NULL,
  pricing_version text NOT NULL,
  outcome text NOT NULL CHECK (outcome IN ('apply','restore')),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY (tenant_id, request_id),
  UNIQUE (tenant_id, revision)
);

CREATE TABLE archava_owner.config_audit (
  tenant_id text NOT NULL,
  revision bigint NOT NULL,
  actor text NOT NULL,
  action text NOT NULL CHECK (action IN ('apply','restore')),
  plan_hash text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY (tenant_id, revision)
);

ALTER TABLE archava_owner.owner_grants ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_owner.owner_grants FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_owner.config_revisions ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_owner.config_revisions FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_owner.apply_receipts ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_owner.apply_receipts FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_owner.config_audit ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_owner.config_audit FORCE ROW LEVEL SECURITY;

CREATE POLICY owner_tenant_grants ON archava_owner.owner_grants
 USING (tenant_id = current_setting('archava.tenant_id', true))
 WITH CHECK (tenant_id = current_setting('archava.tenant_id', true));
CREATE POLICY owner_tenant_revisions ON archava_owner.config_revisions
 USING (tenant_id = current_setting('archava.tenant_id', true))
 WITH CHECK (tenant_id = current_setting('archava.tenant_id', true));
CREATE POLICY owner_tenant_receipts ON archava_owner.apply_receipts
 USING (tenant_id = current_setting('archava.tenant_id', true))
 WITH CHECK (tenant_id = current_setting('archava.tenant_id', true));
CREATE POLICY owner_tenant_audit ON archava_owner.config_audit
 USING (tenant_id = current_setting('archava.tenant_id', true))
 WITH CHECK (tenant_id = current_setting('archava.tenant_id', true));
