CREATE TABLE archava_organizations.memberships (
  tenant_id text NOT NULL,
  principal_id text NOT NULL,
  display_name text NOT NULL DEFAULT '' CHECK(length(display_name) <= 254),
  kind text NOT NULL CHECK(kind IN ('human', 'service')),
  permissions text[] NOT NULL CHECK(permissions <@ ARRAY['organizations.read', 'organizations.write', 'affiliations.read', 'affiliations.write', 'audit.read']::text[]),
  scope text NOT NULL CHECK(scope IN ('all', 'assigned')),
  active boolean NOT NULL DEFAULT true,
  PRIMARY KEY(tenant_id, principal_id)
);

CREATE TABLE archava_organizations.organizations (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  name text NOT NULL CHECK(length(name) BETWEEN 1 AND 160),
  legal_name text CHECK(legal_name IS NULL OR (length(legal_name) BETWEEN 1 AND 160)),
  tax_id text CHECK(tax_id IS NULL OR (length(tax_id) BETWEEN 1 AND 50)),
  website text CHECK(website IS NULL OR (length(website) BETWEEN 1 AND 200)),
  industry text CHECK(industry IS NULL OR (length(industry) BETWEEN 1 AND 100)),
  billing_address text CHECK(billing_address IS NULL OR (length(billing_address) BETWEEN 1 AND 500)),
  state text NOT NULL DEFAULT 'active' CHECK(state IN ('active', 'archived')),
  owner_id text NOT NULL,
  revision integer NOT NULL DEFAULT 1 CHECK(revision > 0),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, id),
  FOREIGN KEY(tenant_id, owner_id) REFERENCES archava_organizations.memberships(tenant_id, principal_id)
);

CREATE INDEX org_list ON archava_organizations.organizations(tenant_id, state, id);
CREATE INDEX org_owner ON archava_organizations.organizations(tenant_id, owner_id, id);

CREATE TABLE archava_organizations.affiliations (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  organization_id uuid NOT NULL,
  contact_id uuid NOT NULL,
  role text NOT NULL CHECK(length(role) BETWEEN 1 AND 80),
  title text CHECK(title IS NULL OR (length(title) BETWEEN 1 AND 100)),
  department text CHECK(department IS NULL OR (length(department) BETWEEN 1 AND 80)),
  is_primary boolean NOT NULL DEFAULT false,
  status text NOT NULL DEFAULT 'active' CHECK(status IN ('active', 'inactive')),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, id),
  FOREIGN KEY(tenant_id, organization_id) REFERENCES archava_organizations.organizations(tenant_id, id)
);

CREATE INDEX org_affiliations ON archava_organizations.affiliations(tenant_id, organization_id, status, id);
CREATE INDEX aff_contact ON archava_organizations.affiliations(tenant_id, contact_id, organization_id);
CREATE UNIQUE INDEX org_contact_role ON archava_organizations.affiliations(tenant_id, organization_id, contact_id, role);
CREATE UNIQUE INDEX org_primary_active ON archava_organizations.affiliations(tenant_id, organization_id) WHERE is_primary = true AND status = 'active';

CREATE TABLE archava_organizations.commands (
  tenant_id text NOT NULL,
  request_id uuid NOT NULL,
  actor_id text NOT NULL,
  fingerprint text NOT NULL,
  receipt jsonb,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, request_id),
  FOREIGN KEY(tenant_id, actor_id) REFERENCES archava_organizations.memberships(tenant_id, principal_id)
);

CREATE TABLE archava_organizations.audit (
  tenant_id text NOT NULL,
  request_id uuid NOT NULL,
  actor_id text NOT NULL,
  operation text NOT NULL,
  organization_id uuid NOT NULL,
  affiliation_id uuid,
  revision integer NOT NULL CHECK(revision > 0),
  reason text CHECK(reason IS NULL OR length(reason) <= 2000),
  occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, request_id),
  FOREIGN KEY(tenant_id, request_id) REFERENCES archava_organizations.commands(tenant_id, request_id)
);

CREATE INDEX org_history ON archava_organizations.audit(tenant_id, organization_id, revision DESC);

CREATE FUNCTION archava_organizations.immutable_history() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  RAISE EXCEPTION 'Organization audit history is immutable';
END
$$;

CREATE TRIGGER audit_immutable BEFORE UPDATE OR DELETE ON archava_organizations.audit FOR EACH ROW EXECUTE FUNCTION archava_organizations.immutable_history();

CREATE FUNCTION archava_organizations.guard_command() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF TG_OP='DELETE' OR OLD.receipt IS NOT NULL OR NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.request_id IS DISTINCT FROM OLD.request_id OR NEW.actor_id IS DISTINCT FROM OLD.actor_id OR NEW.fingerprint IS DISTINCT FROM OLD.fingerprint OR NEW.created_at IS DISTINCT FROM OLD.created_at THEN
    RAISE EXCEPTION 'Organization command is immutable';
  END IF;
  RETURN NEW;
END
$$;

CREATE TRIGGER command_guard BEFORE UPDATE OR DELETE ON archava_organizations.commands FOR EACH ROW EXECUTE FUNCTION archava_organizations.guard_command();

DO $$ DECLARE owned text; BEGIN
  FOREACH owned IN ARRAY ARRAY['memberships', 'organizations', 'affiliations', 'commands', 'audit'] LOOP
    EXECUTE format('ALTER TABLE archava_organizations.%I ENABLE ROW LEVEL SECURITY', owned);
    EXECUTE format('ALTER TABLE archava_organizations.%I FORCE ROW LEVEL SECURITY', owned);
    EXECUTE format('CREATE POLICY org_tenant ON archava_organizations.%I USING(tenant_id=current_setting(''archava.tenant_id'',true)) WITH CHECK(tenant_id=current_setting(''archava.tenant_id'',true))', owned);
  END LOOP;
END $$;

CREATE FUNCTION archava_organizations.lock_membership(wanted_tenant text, wanted_principal text)
RETURNS SETOF archava_organizations.memberships LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog AS $$
  SELECT * FROM archava_organizations.memberships WHERE tenant_id=wanted_tenant AND principal_id=wanted_principal AND wanted_tenant=current_setting('archava.tenant_id',true) FOR SHARE
$$;

REVOKE ALL ON FUNCTION archava_organizations.lock_membership(text, text) FROM PUBLIC;
