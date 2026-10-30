CREATE TABLE archava_capacity.memberships (
  tenant_id text NOT NULL,
  principal_id text NOT NULL,
  display_name text NOT NULL DEFAULT '' CHECK(length(display_name) <= 254),
  kind text NOT NULL CHECK(kind IN ('human', 'service')),
  permissions text[] NOT NULL CHECK(permissions <@ ARRAY['capacity.read', 'capacity.write', 'capacity.allocate', 'audit.read']::text[]),
  scope text NOT NULL CHECK(scope IN ('all', 'assigned')),
  active boolean NOT NULL DEFAULT true,
  PRIMARY KEY(tenant_id, principal_id)
);

CREATE TABLE archava_capacity.pools (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  code text NOT NULL CHECK(code ~ '^[A-Z0-9_]{2,40}$'),
  name text NOT NULL CHECK(length(name) BETWEEN 1 AND 160),
  unit text NOT NULL DEFAULT 'hours' CHECK(unit IN ('hours', 'days', 'slots')),
  weekly_capacity numeric(12,2) NOT NULL CHECK(weekly_capacity > 0),
  max_concurrent_jobs integer NOT NULL DEFAULT 3 CHECK(max_concurrent_jobs > 0),
  state text NOT NULL DEFAULT 'active' CHECK(state IN ('active', 'paused', 'archived')),
  owner_id text NOT NULL,
  revision integer NOT NULL DEFAULT 1 CHECK(revision > 0),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, id),
  FOREIGN KEY(tenant_id, owner_id) REFERENCES archava_capacity.memberships(tenant_id, principal_id),
  UNIQUE(tenant_id, code)
);

CREATE INDEX cap_pool_list ON archava_capacity.pools(tenant_id, state, id);
CREATE INDEX cap_pool_owner ON archava_capacity.pools(tenant_id, owner_id, id);

CREATE TABLE archava_capacity.allocations (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  pool_id uuid NOT NULL,
  workload_id text NOT NULL CHECK(length(workload_id) BETWEEN 1 AND 100),
  workload_title text NOT NULL CHECK(length(workload_title) BETWEEN 1 AND 160),
  required_units numeric(12,2) NOT NULL CHECK(required_units > 0),
  remaining_units numeric(12,2) NOT NULL CHECK(remaining_units >= 0),
  scheduled_start_at timestamptz NOT NULL,
  scheduled_finish_at timestamptz NOT NULL,
  state text NOT NULL DEFAULT 'scheduled' CHECK(state IN ('scheduled', 'in_progress', 'completed', 'cancelled')),
  owner_id text NOT NULL,
  revision integer NOT NULL DEFAULT 1 CHECK(revision > 0),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, id),
  FOREIGN KEY(tenant_id, pool_id) REFERENCES archava_capacity.pools(tenant_id, id),
  FOREIGN KEY(tenant_id, owner_id) REFERENCES archava_capacity.memberships(tenant_id, principal_id)
);

CREATE INDEX cap_alloc_queue ON archava_capacity.allocations(tenant_id, pool_id, state, scheduled_finish_at);
CREATE INDEX cap_alloc_workload ON archava_capacity.allocations(tenant_id, workload_id);

CREATE TABLE archava_capacity.commands (
  tenant_id text NOT NULL,
  command_id uuid NOT NULL,
  actor_id text NOT NULL,
  fingerprint text NOT NULL,
  receipt jsonb,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, command_id),
  FOREIGN KEY(tenant_id, actor_id) REFERENCES archava_capacity.memberships(tenant_id, principal_id)
);

CREATE TABLE archava_capacity.audit (
  tenant_id text NOT NULL,
  command_id uuid NOT NULL,
  actor_id text NOT NULL,
  operation text NOT NULL,
  pool_id uuid NOT NULL,
  allocation_id uuid,
  revision integer NOT NULL CHECK(revision > 0),
  reason text CHECK(reason IS NULL OR length(reason) <= 2000),
  occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, command_id),
  FOREIGN KEY(tenant_id, command_id) REFERENCES archava_capacity.commands(tenant_id, command_id)
);

CREATE INDEX cap_history ON archava_capacity.audit(tenant_id, pool_id, revision DESC);

CREATE FUNCTION archava_capacity.immutable_history() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  RAISE EXCEPTION 'Capacity audit history is immutable';
END
$$;

CREATE TRIGGER audit_immutable BEFORE UPDATE OR DELETE ON archava_capacity.audit FOR EACH ROW EXECUTE FUNCTION archava_capacity.immutable_history();

CREATE FUNCTION archava_capacity.guard_command() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF TG_OP='DELETE' OR OLD.receipt IS NOT NULL OR NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.command_id IS DISTINCT FROM OLD.command_id OR NEW.actor_id IS DISTINCT FROM OLD.actor_id OR NEW.fingerprint IS DISTINCT FROM OLD.fingerprint OR NEW.created_at IS DISTINCT FROM OLD.created_at THEN
    RAISE EXCEPTION 'Capacity command is immutable';
  END IF;
  RETURN NEW;
END
$$;

CREATE TRIGGER command_guard BEFORE UPDATE OR DELETE ON archava_capacity.commands FOR EACH ROW EXECUTE FUNCTION archava_capacity.guard_command();

DO $$ DECLARE owned text; BEGIN
  FOREACH owned IN ARRAY ARRAY['memberships', 'pools', 'allocations', 'commands', 'audit'] LOOP
    EXECUTE format('ALTER TABLE archava_capacity.%I ENABLE ROW LEVEL SECURITY', owned);
    EXECUTE format('ALTER TABLE archava_capacity.%I FORCE ROW LEVEL SECURITY', owned);
    EXECUTE format('CREATE POLICY cap_tenant ON archava_capacity.%I USING(tenant_id=current_setting(''archava.tenant_id'',true)) WITH CHECK(tenant_id=current_setting(''archava.tenant_id'',true))', owned);
  END LOOP;
END $$;

CREATE FUNCTION archava_capacity.lock_membership(wanted_tenant text, wanted_principal text)
RETURNS SETOF archava_capacity.memberships LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog AS $$
  SELECT * FROM archava_capacity.memberships WHERE tenant_id=wanted_tenant AND principal_id=wanted_principal AND wanted_tenant=current_setting('archava.tenant_id',true) FOR SHARE
$$;

REVOKE ALL ON FUNCTION archava_capacity.lock_membership(text, text) FROM PUBLIC;
