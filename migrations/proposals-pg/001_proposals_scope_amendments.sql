CREATE TABLE archava_proposals.memberships (
  tenant_id text NOT NULL,
  principal_id text NOT NULL,
  display_name text NOT NULL DEFAULT '' CHECK(length(display_name) <= 254),
  kind text NOT NULL CHECK(kind IN ('human', 'service')),
  permissions text[] NOT NULL CHECK(permissions <@ ARRAY['proposals.read', 'proposals.write', 'proposals.decide', 'changerequests.read', 'changerequests.write', 'changerequests.decide', 'audit.read']::text[]),
  scope text NOT NULL CHECK(scope IN ('all', 'assigned')),
  active boolean NOT NULL DEFAULT true,
  PRIMARY KEY(tenant_id, principal_id)
);

CREATE TABLE archava_proposals.proposals (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  title text NOT NULL CHECK(length(title) BETWEEN 1 AND 160),
  summary text NOT NULL CHECK(length(summary) BETWEEN 1 AND 2000),
  organization_id uuid,
  contact_id text,
  lead_id text,
  currency text NOT NULL DEFAULT 'IDR' CHECK(currency ~ '^[A-Z]{3}$'),
  total_estimated_value bigint NOT NULL DEFAULT 0 CHECK(total_estimated_value >= 0),
  state text NOT NULL DEFAULT 'draft' CHECK(state IN ('draft', 'submitted', 'accepted', 'rejected', 'withdrawn')),
  current_scope_version integer NOT NULL DEFAULT 1 CHECK(current_scope_version > 0),
  accepted_at timestamptz,
  accepted_by text,
  linked_project_id uuid,
  owner_id text NOT NULL,
  revision integer NOT NULL DEFAULT 1 CHECK(revision > 0),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, id),
  FOREIGN KEY(tenant_id, owner_id) REFERENCES archava_proposals.memberships(tenant_id, principal_id)
);

CREATE INDEX prop_list ON archava_proposals.proposals(tenant_id, state, id);
CREATE INDEX prop_owner ON archava_proposals.proposals(tenant_id, owner_id, id);
CREATE INDEX prop_org ON archava_proposals.proposals(tenant_id, organization_id, id) WHERE organization_id IS NOT NULL;

CREATE TABLE archava_proposals.scope_items (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  proposal_id uuid NOT NULL,
  scope_version integer NOT NULL CHECK(scope_version > 0),
  title text NOT NULL CHECK(length(title) BETWEEN 1 AND 160),
  description text NOT NULL CHECK(length(description) BETWEEN 1 AND 2000),
  estimated_days integer CHECK(estimated_days IS NULL OR estimated_days >= 0),
  estimated_value bigint NOT NULL DEFAULT 0 CHECK(estimated_value >= 0),
  deliverable_type text NOT NULL DEFAULT 'service' CHECK(deliverable_type IN ('milestone', 'service', 'license', 'contingency')),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, id),
  FOREIGN KEY(tenant_id, proposal_id) REFERENCES archava_proposals.proposals(tenant_id, id)
);

CREATE INDEX prop_items ON archava_proposals.scope_items(tenant_id, proposal_id, scope_version);

CREATE TABLE archava_proposals.scope_versions (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  proposal_id uuid NOT NULL,
  scope_version integer NOT NULL CHECK(scope_version > 0),
  change_request_id uuid,
  summary text NOT NULL CHECK(length(summary) BETWEEN 1 AND 2000),
  total_value bigint NOT NULL DEFAULT 0 CHECK(total_value >= 0),
  total_days integer NOT NULL DEFAULT 0 CHECK(total_days >= 0),
  confirmed_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  confirmed_by text NOT NULL,
  items jsonb NOT NULL,
  PRIMARY KEY(tenant_id, id),
  FOREIGN KEY(tenant_id, proposal_id) REFERENCES archava_proposals.proposals(tenant_id, id),
  UNIQUE(tenant_id, proposal_id, scope_version)
);

CREATE INDEX prop_versions ON archava_proposals.scope_versions(tenant_id, proposal_id, scope_version);

CREATE TABLE archava_proposals.change_requests (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  proposal_id uuid NOT NULL,
  change_number integer NOT NULL CHECK(change_number > 0),
  title text NOT NULL CHECK(length(title) BETWEEN 1 AND 160),
  reason text NOT NULL CHECK(length(reason) BETWEEN 1 AND 2000),
  delta_summary text NOT NULL CHECK(length(delta_summary) BETWEEN 1 AND 2000),
  time_delta_days integer NOT NULL DEFAULT 0,
  cost_delta_value bigint NOT NULL DEFAULT 0,
  resulting_scope_version integer,
  state text NOT NULL DEFAULT 'draft' CHECK(state IN ('draft', 'submitted', 'approved', 'rejected', 'withdrawn')),
  submitted_at timestamptz,
  decided_at timestamptz,
  decided_by text,
  owner_id text NOT NULL,
  revision integer NOT NULL DEFAULT 1 CHECK(revision > 0),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, id),
  FOREIGN KEY(tenant_id, proposal_id) REFERENCES archava_proposals.proposals(tenant_id, id),
  UNIQUE(tenant_id, proposal_id, change_number)
);

CREATE INDEX prop_cr ON archava_proposals.change_requests(tenant_id, proposal_id, change_number);
CREATE UNIQUE INDEX cr_active_per_proposal ON archava_proposals.change_requests(tenant_id, proposal_id) WHERE state IN ('draft', 'submitted');

CREATE TABLE archava_proposals.commands (
  tenant_id text NOT NULL,
  command_id uuid NOT NULL,
  actor_id text NOT NULL,
  fingerprint text NOT NULL,
  receipt jsonb,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, command_id),
  FOREIGN KEY(tenant_id, actor_id) REFERENCES archava_proposals.memberships(tenant_id, principal_id)
);

CREATE TABLE archava_proposals.audit (
  tenant_id text NOT NULL,
  command_id uuid NOT NULL,
  actor_id text NOT NULL,
  operation text NOT NULL,
  proposal_id uuid NOT NULL,
  change_request_id uuid,
  revision integer NOT NULL CHECK(revision > 0),
  reason text CHECK(reason IS NULL OR length(reason) <= 2000),
  occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, command_id),
  FOREIGN KEY(tenant_id, command_id) REFERENCES archava_proposals.commands(tenant_id, command_id)
);

CREATE INDEX prop_history ON archava_proposals.audit(tenant_id, proposal_id, revision DESC);

CREATE FUNCTION archava_proposals.immutable_history() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  RAISE EXCEPTION 'Proposal audit history is immutable';
END
$$;

CREATE TRIGGER audit_immutable BEFORE UPDATE OR DELETE ON archava_proposals.audit FOR EACH ROW EXECUTE FUNCTION archava_proposals.immutable_history();

CREATE FUNCTION archava_proposals.guard_command() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF TG_OP='DELETE' OR OLD.receipt IS NOT NULL OR NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.command_id IS DISTINCT FROM OLD.command_id OR NEW.actor_id IS DISTINCT FROM OLD.actor_id OR NEW.fingerprint IS DISTINCT FROM OLD.fingerprint OR NEW.created_at IS DISTINCT FROM OLD.created_at THEN
    RAISE EXCEPTION 'Proposal command is immutable';
  END IF;
  RETURN NEW;
END
$$;

CREATE TRIGGER command_guard BEFORE UPDATE OR DELETE ON archava_proposals.commands FOR EACH ROW EXECUTE FUNCTION archava_proposals.guard_command();

DO $$ DECLARE owned text; BEGIN
  FOREACH owned IN ARRAY ARRAY['memberships', 'proposals', 'scope_items', 'scope_versions', 'change_requests', 'commands', 'audit'] LOOP
    EXECUTE format('ALTER TABLE archava_proposals.%I ENABLE ROW LEVEL SECURITY', owned);
    EXECUTE format('ALTER TABLE archava_proposals.%I FORCE ROW LEVEL SECURITY', owned);
    EXECUTE format('CREATE POLICY prop_tenant ON archava_proposals.%I USING(tenant_id=current_setting(''archava.tenant_id'',true)) WITH CHECK(tenant_id=current_setting(''archava.tenant_id'',true))', owned);
  END LOOP;
END $$;

CREATE FUNCTION archava_proposals.lock_membership(wanted_tenant text, wanted_principal text)
RETURNS SETOF archava_proposals.memberships LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog AS $$
  SELECT * FROM archava_proposals.memberships WHERE tenant_id=wanted_tenant AND principal_id=wanted_principal AND wanted_tenant=current_setting('archava.tenant_id',true) FOR SHARE
$$;

REVOKE ALL ON FUNCTION archava_proposals.lock_membership(text, text) FROM PUBLIC;
