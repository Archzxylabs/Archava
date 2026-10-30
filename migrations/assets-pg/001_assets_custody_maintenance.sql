CREATE TABLE archava_assets.memberships (
  tenant_id text NOT NULL,
  principal_id text NOT NULL,
  display_name text NOT NULL DEFAULT '' CHECK(length(display_name) <= 254),
  kind text NOT NULL CHECK(kind IN ('human', 'service')),
  permissions text[] NOT NULL CHECK(permissions <@ ARRAY['assets.read', 'assets.write', 'assets.handover', 'assets.maintenance', 'audit.read']::text[]),
  scope text NOT NULL CHECK(scope IN ('all', 'assigned')),
  active boolean NOT NULL DEFAULT true,
  PRIMARY KEY(tenant_id, principal_id)
);

CREATE TABLE archava_assets.assets (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  code text NOT NULL CHECK(length(code) BETWEEN 1 AND 50),
  name text NOT NULL CHECK(length(name) BETWEEN 1 AND 160),
  category text NOT NULL CHECK(length(category) BETWEEN 1 AND 80),
  serial_number text CHECK(serial_number IS NULL OR (length(serial_number) BETWEEN 1 AND 80)),
  model text CHECK(model IS NULL OR (length(model) BETWEEN 1 AND 80)),
  ownership text NOT NULL CHECK(ownership IN ('business_owned', 'customer_owned')),
  customer_contact_id text CHECK(customer_contact_id IS NULL OR (length(customer_contact_id) BETWEEN 1 AND 80)),
  status text NOT NULL DEFAULT 'available' CHECK(status IN ('available', 'in_custody', 'under_maintenance', 'retired', 'returned_to_customer')),
  condition text NOT NULL DEFAULT 'good' CHECK(condition IN ('excellent', 'good', 'fair', 'damaged', 'unserviceable')),
  current_custodian_id text CHECK(current_custodian_id IS NULL OR (length(current_custodian_id) BETWEEN 1 AND 80)),
  current_location text CHECK(current_location IS NULL OR (length(current_location) BETWEEN 1 AND 160)),
  owner_id text NOT NULL,
  revision integer NOT NULL DEFAULT 1 CHECK(revision > 0),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, id),
  FOREIGN KEY(tenant_id, owner_id) REFERENCES archava_assets.memberships(tenant_id, principal_id),
  CONSTRAINT customer_owned_contact CHECK (
    ownership != 'customer_owned' OR (customer_contact_id IS NOT NULL AND length(trim(customer_contact_id)) > 0)
  )
);

CREATE UNIQUE INDEX asset_tenant_code ON archava_assets.assets(tenant_id, code);
CREATE INDEX asset_list ON archava_assets.assets(tenant_id, status, id);
CREATE INDEX asset_owner ON archava_assets.assets(tenant_id, owner_id, id);
CREATE INDEX asset_category ON archava_assets.assets(tenant_id, category, id);
CREATE INDEX asset_custodian ON archava_assets.assets(tenant_id, current_custodian_id) WHERE current_custodian_id IS NOT NULL;

CREATE TABLE archava_assets.custody_events (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  asset_id uuid NOT NULL,
  event_type text NOT NULL CHECK(event_type IN ('dispatch', 'checkin', 'transfer', 'service_intake', 'customer_return')),
  from_custodian_id text CHECK(from_custodian_id IS NULL OR length(from_custodian_id) <= 80),
  to_custodian_id text CHECK(to_custodian_id IS NULL OR length(to_custodian_id) <= 80),
  condition_before text NOT NULL CHECK(condition_before IN ('excellent', 'good', 'fair', 'damaged', 'unserviceable')),
  condition_after text NOT NULL CHECK(condition_after IN ('excellent', 'good', 'fair', 'damaged', 'unserviceable')),
  location text CHECK(location IS NULL OR length(location) <= 160),
  notes text CHECK(notes IS NULL OR length(notes) <= 500),
  recorded_by text NOT NULL,
  recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, id),
  FOREIGN KEY(tenant_id, asset_id) REFERENCES archava_assets.assets(tenant_id, id)
);

CREATE INDEX asset_custody_history ON archava_assets.custody_events(tenant_id, asset_id, recorded_at DESC);

CREATE TABLE archava_assets.maintenance_records (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  asset_id uuid NOT NULL,
  service_type text NOT NULL CHECK(service_type IN ('preventive', 'corrective', 'inspection')),
  description text NOT NULL CHECK(length(description) BETWEEN 1 AND 160),
  parts_used text CHECK(parts_used IS NULL OR length(parts_used) <= 500),
  condition_after text NOT NULL CHECK(condition_after IN ('excellent', 'good', 'fair', 'damaged', 'unserviceable')),
  performed_by text NOT NULL,
  recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, id),
  FOREIGN KEY(tenant_id, asset_id) REFERENCES archava_assets.assets(tenant_id, id)
);

CREATE INDEX asset_maintenance_history ON archava_assets.maintenance_records(tenant_id, asset_id, recorded_at DESC);

CREATE TABLE archava_assets.commands (
  tenant_id text NOT NULL,
  request_id uuid NOT NULL,
  actor_id text NOT NULL,
  fingerprint text NOT NULL,
  receipt jsonb,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, request_id),
  FOREIGN KEY(tenant_id, actor_id) REFERENCES archava_assets.memberships(tenant_id, principal_id)
);

CREATE TABLE archava_assets.audit (
  tenant_id text NOT NULL,
  request_id uuid NOT NULL,
  actor_id text NOT NULL,
  operation text NOT NULL,
  asset_id uuid NOT NULL,
  revision integer NOT NULL CHECK(revision > 0),
  reason text CHECK(reason IS NULL OR length(reason) <= 2000),
  occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, request_id),
  FOREIGN KEY(tenant_id, request_id) REFERENCES archava_assets.commands(tenant_id, request_id)
);

CREATE INDEX asset_audit_history ON archava_assets.audit(tenant_id, asset_id, revision DESC);

CREATE FUNCTION archava_assets.immutable_history() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  RAISE EXCEPTION 'Asset audit history is immutable';
END
$$;

CREATE TRIGGER audit_immutable BEFORE UPDATE OR DELETE ON archava_assets.audit FOR EACH ROW EXECUTE FUNCTION archava_assets.immutable_history();

CREATE FUNCTION archava_assets.guard_command() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF TG_OP='DELETE' OR OLD.receipt IS NOT NULL OR NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.request_id IS DISTINCT FROM OLD.request_id OR NEW.actor_id IS DISTINCT FROM OLD.actor_id OR NEW.fingerprint IS DISTINCT FROM OLD.fingerprint OR NEW.created_at IS DISTINCT FROM OLD.created_at THEN
    RAISE EXCEPTION 'Asset command is immutable';
  END IF;
  RETURN NEW;
END
$$;

CREATE TRIGGER command_guard BEFORE UPDATE OR DELETE ON archava_assets.commands FOR EACH ROW EXECUTE FUNCTION archava_assets.guard_command();

DO $$ DECLARE owned text; BEGIN
  FOREACH owned IN ARRAY ARRAY['memberships', 'assets', 'custody_events', 'maintenance_records', 'commands', 'audit'] LOOP
    EXECUTE format('ALTER TABLE archava_assets.%I ENABLE ROW LEVEL SECURITY', owned);
    EXECUTE format('ALTER TABLE archava_assets.%I FORCE ROW LEVEL SECURITY', owned);
    EXECUTE format('CREATE POLICY asset_tenant ON archava_assets.%I USING(tenant_id=current_setting(''archava.tenant_id'',true)) WITH CHECK(tenant_id=current_setting(''archava.tenant_id'',true))', owned);
  END LOOP;
END $$;

CREATE FUNCTION archava_assets.lock_membership(wanted_tenant text, wanted_principal text)
RETURNS SETOF archava_assets.memberships LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog AS $$
  SELECT * FROM archava_assets.memberships WHERE tenant_id=wanted_tenant AND principal_id=wanted_principal AND wanted_tenant=current_setting('archava.tenant_id',true) FOR SHARE
$$;

REVOKE ALL ON FUNCTION archava_assets.lock_membership(text, text) FROM PUBLIC;
