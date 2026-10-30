CREATE TABLE archava_fulfillment.memberships (
  tenant_id text NOT NULL,
  principal_id text NOT NULL,
  display_name text NOT NULL DEFAULT '' CHECK(length(display_name) <= 254),
  kind text NOT NULL CHECK(kind IN ('human', 'service')),
  permissions text[] NOT NULL CHECK(permissions <@ ARRAY['fulfillment.read', 'fulfillment.write', 'fulfillment.dispatch', 'fulfillment.produce', 'audit.read']::text[]),
  scope text NOT NULL CHECK(scope IN ('all', 'assigned')),
  active boolean NOT NULL DEFAULT true,
  PRIMARY KEY(tenant_id, principal_id)
);

CREATE TABLE archava_fulfillment.jobs (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  job_code text NOT NULL CHECK(length(job_code) BETWEEN 1 AND 50),
  kind text NOT NULL CHECK(kind IN ('dispatch', 'production')),
  title text NOT NULL CHECK(length(title) BETWEEN 1 AND 200),
  owner_ref text CHECK(owner_ref IS NULL OR length(owner_ref) <= 80),
  status text NOT NULL DEFAULT 'draft' CHECK(status IN ('draft', 'released', 'in_progress', 'completed', 'canceled')),
  consumed_count integer NOT NULL DEFAULT 0 CHECK(consumed_count >= 0),
  produced_count integer NOT NULL DEFAULT 0 CHECK(produced_count >= 0),
  scrap_count integer NOT NULL DEFAULT 0 CHECK(scrap_count >= 0),
  unreconciled_count integer NOT NULL DEFAULT 0 CHECK(unreconciled_count >= 0),
  owner_id text NOT NULL,
  revision integer NOT NULL DEFAULT 1 CHECK(revision > 0),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, id),
  FOREIGN KEY(tenant_id, owner_id) REFERENCES archava_fulfillment.memberships(tenant_id, principal_id),
  CONSTRAINT job_code_unique UNIQUE (tenant_id, job_code),
  CONSTRAINT job_reconciliation_gate CHECK (status <> 'completed' OR unreconciled_count = 0)
);

CREATE INDEX job_list ON archava_fulfillment.jobs(tenant_id, status, created_at, id);
CREATE INDEX job_code ON archava_fulfillment.jobs(tenant_id, job_code);
CREATE INDEX job_owner_ref ON archava_fulfillment.jobs(tenant_id, owner_ref, id);

CREATE TABLE archava_fulfillment.job_lines (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  job_id uuid NOT NULL,
  direction text NOT NULL CHECK(direction IN ('consume', 'produce')),
  item_ref text NOT NULL CHECK(length(item_ref) BETWEEN 1 AND 40),
  quantity integer NOT NULL CHECK(quantity > 0),
  disposition text NOT NULL DEFAULT 'normal' CHECK(disposition IN ('normal', 'scrap')),
  evidence_ref text CHECK(evidence_ref IS NULL OR length(evidence_ref) <= 300),
  recorded_by text NOT NULL,
  recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, id),
  FOREIGN KEY(tenant_id, job_id) REFERENCES archava_fulfillment.jobs(tenant_id, id)
);

CREATE INDEX job_line_job ON archava_fulfillment.job_lines(tenant_id, job_id, recorded_at, id);

CREATE TABLE archava_fulfillment.findings (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  job_id uuid NOT NULL,
  category text NOT NULL CHECK(category IN ('condition', 'quantity', 'delivery')),
  disposition text NOT NULL CHECK(disposition IN ('accepted', 'partial', 'rejected', 'lost', 'damaged')),
  quantity integer CHECK(quantity IS NULL OR quantity > 0),
  reason text NOT NULL CHECK(length(reason) BETWEEN 1 AND 500),
  evidence_ref text CHECK(evidence_ref IS NULL OR length(evidence_ref) <= 300),
  reconciles_id uuid,
  recorded_by text NOT NULL,
  recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, id),
  FOREIGN KEY(tenant_id, job_id) REFERENCES archava_fulfillment.jobs(tenant_id, id),
  CONSTRAINT finding_reconciles_requires_acceptance CHECK (reconciles_id IS NULL OR disposition = 'accepted')
);

CREATE INDEX finding_job ON archava_fulfillment.findings(tenant_id, job_id, recorded_at, id);
CREATE UNIQUE INDEX finding_reconciles_once ON archava_fulfillment.findings(tenant_id, reconciles_id) WHERE reconciles_id IS NOT NULL;

CREATE TABLE archava_fulfillment.custody_events (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  job_id uuid NOT NULL,
  event text NOT NULL CHECK(event IN ('received', 'stored', 'picked', 'packed', 'dispatched', 'delivered', 'returned')),
  from_ref text CHECK(from_ref IS NULL OR length(from_ref) <= 80),
  to_ref text CHECK(to_ref IS NULL OR length(to_ref) <= 80),
  evidence_ref text CHECK(evidence_ref IS NULL OR length(evidence_ref) <= 300),
  note text CHECK(note IS NULL OR length(note) <= 500),
  recorded_by text NOT NULL,
  recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, id),
  FOREIGN KEY(tenant_id, job_id) REFERENCES archava_fulfillment.jobs(tenant_id, id)
);

CREATE INDEX custody_job ON archava_fulfillment.custody_events(tenant_id, job_id, recorded_at, id);

CREATE TABLE archava_fulfillment.commands (
  tenant_id text NOT NULL,
  request_id uuid NOT NULL,
  actor_id text NOT NULL,
  fingerprint text NOT NULL,
  receipt jsonb,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, request_id),
  FOREIGN KEY(tenant_id, actor_id) REFERENCES archava_fulfillment.memberships(tenant_id, principal_id)
);

CREATE TABLE archava_fulfillment.audit (
  tenant_id text NOT NULL,
  request_id uuid NOT NULL,
  actor_id text NOT NULL,
  operation text NOT NULL,
  job_id uuid,
  revision integer NOT NULL CHECK(revision > 0),
  reason text CHECK(reason IS NULL OR length(reason) <= 2000),
  occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, request_id),
  FOREIGN KEY(tenant_id, request_id) REFERENCES archava_fulfillment.commands(tenant_id, request_id)
);

CREATE INDEX fulfillment_audit_history ON archava_fulfillment.audit(tenant_id, job_id, revision DESC);

CREATE FUNCTION archava_fulfillment.immutable_evidence() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  RAISE EXCEPTION 'Fulfillment evidence is append-only';
END
$$;

CREATE TRIGGER lines_immutable BEFORE UPDATE OR DELETE ON archava_fulfillment.job_lines FOR EACH ROW EXECUTE FUNCTION archava_fulfillment.immutable_evidence();
CREATE TRIGGER findings_immutable BEFORE UPDATE OR DELETE ON archava_fulfillment.findings FOR EACH ROW EXECUTE FUNCTION archava_fulfillment.immutable_evidence();
CREATE TRIGGER custody_immutable BEFORE UPDATE OR DELETE ON archava_fulfillment.custody_events FOR EACH ROW EXECUTE FUNCTION archava_fulfillment.immutable_evidence();

CREATE FUNCTION archava_fulfillment.immutable_history() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  RAISE EXCEPTION 'Fulfillment audit history is immutable';
END
$$;

CREATE TRIGGER audit_immutable BEFORE UPDATE OR DELETE ON archava_fulfillment.audit FOR EACH ROW EXECUTE FUNCTION archava_fulfillment.immutable_history();

CREATE FUNCTION archava_fulfillment.guard_command() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF TG_OP='DELETE' OR OLD.receipt IS NOT NULL OR NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.request_id IS DISTINCT FROM OLD.request_id OR NEW.actor_id IS DISTINCT FROM OLD.actor_id OR NEW.fingerprint IS DISTINCT FROM OLD.fingerprint OR NEW.created_at IS DISTINCT FROM OLD.created_at THEN
    RAISE EXCEPTION 'Fulfillment command is immutable';
  END IF;
  RETURN NEW;
END
$$;

CREATE TRIGGER command_guard BEFORE UPDATE OR DELETE ON archava_fulfillment.commands FOR EACH ROW EXECUTE FUNCTION archava_fulfillment.guard_command();

-- Custody and production traceability require retained jobs; evidence cannot dangle.
CREATE FUNCTION archava_fulfillment.guard_job_retention() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF TG_OP='DELETE' THEN
    RAISE EXCEPTION 'Fulfillment jobs are retained for custody and production audit';
  END IF;
  RETURN NEW;
END
$$;

CREATE TRIGGER job_retention BEFORE DELETE ON archava_fulfillment.jobs FOR EACH ROW EXECUTE FUNCTION archava_fulfillment.guard_job_retention();

ALTER TABLE archava_fulfillment.findings
  ADD CONSTRAINT finding_reconciles_target FOREIGN KEY(tenant_id, reconciles_id) REFERENCES archava_fulfillment.findings(tenant_id, id);

DO $$ DECLARE owned text; BEGIN
  FOREACH owned IN ARRAY ARRAY['memberships', 'jobs', 'job_lines', 'findings', 'custody_events', 'commands', 'audit'] LOOP
    EXECUTE format('ALTER TABLE archava_fulfillment.%I ENABLE ROW LEVEL SECURITY', owned);
    EXECUTE format('ALTER TABLE archava_fulfillment.%I FORCE ROW LEVEL SECURITY', owned);
    EXECUTE format('CREATE POLICY fulfillment_tenant ON archava_fulfillment.%I USING(tenant_id=current_setting(''archava.tenant_id'',true)) WITH CHECK(tenant_id=current_setting(''archava.tenant_id'',true))', owned);
  END LOOP;
END $$;

CREATE FUNCTION archava_fulfillment.lock_membership(wanted_tenant text, wanted_principal text)
RETURNS SETOF archava_fulfillment.memberships LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog AS $$
  SELECT * FROM archava_fulfillment.memberships WHERE tenant_id=wanted_tenant AND principal_id=wanted_principal AND wanted_tenant=current_setting('archava.tenant_id',true) FOR SHARE
$$;

REVOKE ALL ON FUNCTION archava_fulfillment.lock_membership(text, text) FROM PUBLIC;
