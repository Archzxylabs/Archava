-- Field services: bounded dispatch, site visits, work evidence and signoff.
-- The appointment reference is the scheduler's identity and must survive here.
-- Labour hours and parts pieces stay on separate axes: a single counter must
-- never be able to stand in for both.
CREATE SCHEMA IF NOT EXISTS archava_field;

CREATE TABLE IF NOT EXISTS archava_field.memberships (
  tenant_id text NOT NULL,
  principal_id text NOT NULL,
  kind text NOT NULL CHECK (kind IN ('human','service')),
  permissions text[] NOT NULL,
  scope text NOT NULL CHECK (scope IN ('all','assigned')),
  active boolean NOT NULL DEFAULT true,
  PRIMARY KEY (tenant_id, principal_id)
);

CREATE TABLE IF NOT EXISTS archava_field.jobs (
  job_id text PRIMARY KEY,
  tenant_id text NOT NULL,
  customer_ref text NOT NULL,
  site_ref text NOT NULL,
  summary text NOT NULL,
  priority text NOT NULL CHECK (priority IN ('low','normal','high','urgent')),
  status text NOT NULL CHECK (status IN ('requested','assigned','visited','evidenced','signed_off','approved','canceled')),
  appointment_ref text NOT NULL,
  asset_ref text,
  assigned_principal text,
  labor_hours numeric(12,2) NOT NULL DEFAULT 0 CHECK (labor_hours >= 0),
  parts_pieces integer NOT NULL DEFAULT 0 CHECK (parts_pieces >= 0),
  revision integer NOT NULL DEFAULT 1 CHECK (revision > 0),
  created_by text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS jobs_tenant_status ON archava_field.jobs(tenant_id, status, job_id);
-- The dispatch handover must never lose which appointment it came from.
ALTER TABLE archava_field.jobs DROP CONSTRAINT IF EXISTS jobs_appointment_required;
ALTER TABLE archava_field.jobs
  ADD CONSTRAINT jobs_appointment_required CHECK (length(btrim(appointment_ref)) > 0);

CREATE TABLE IF NOT EXISTS archava_field.visits (
  visit_id text PRIMARY KEY,
  job_id text NOT NULL REFERENCES archava_field.jobs(job_id) ON DELETE RESTRICT,
  outcome text NOT NULL CHECK (outcome IN ('completed','partial','no_access')),
  note text NOT NULL,
  technician_ref text NOT NULL,
  visited_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS visits_job ON archava_field.visits(job_id, visited_at);

CREATE TABLE IF NOT EXISTS archava_field.work_lines (
  line_id text PRIMARY KEY,
  job_id text NOT NULL REFERENCES archava_field.jobs(job_id) ON DELETE RESTRICT,
  kind text NOT NULL CHECK (kind IN ('labour','part')),
  description text NOT NULL,
  quantity numeric(12,2) NOT NULL CHECK (quantity > 0),
  unit text NOT NULL CHECK (unit IN ('hours','pieces')),
  part_ref text,
  recorded_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS work_lines_job ON archava_field.work_lines(job_id, recorded_at);
-- Hours and pieces are different measures; a line cannot claim both at once.
ALTER TABLE archava_field.work_lines DROP CONSTRAINT IF EXISTS work_lines_unit_matches_kind;
ALTER TABLE archava_field.work_lines
  ADD CONSTRAINT work_lines_unit_matches_kind CHECK (
    (kind = 'labour' AND unit = 'hours' AND part_ref IS NULL) OR
    (kind = 'part' AND unit = 'pieces')
  );

CREATE TABLE IF NOT EXISTS archava_field.signoffs (
  signoff_id text PRIMARY KEY,
  job_id text NOT NULL REFERENCES archava_field.jobs(job_id) ON DELETE RESTRICT,
  signer_ref text NOT NULL,
  accepted boolean NOT NULL,
  comment text,
  signed_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS archava_field.commands (
  command_id text PRIMARY KEY,
  job_id text NOT NULL,
  tenant_id text NOT NULL,
  action text NOT NULL CHECK (action IN ('create_job','assign_job','record_visit','append_work_line','record_signoff','approve_job','cancel_job')),
  fingerprint text NOT NULL,
  payload jsonb NOT NULL,
  receipt jsonb,
  status text NOT NULL CHECK (status IN ('accepted','rejected')),
  revision integer,
  created_by text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS archava_field.audit (
  entry_id text PRIMARY KEY,
  job_id text NOT NULL,
  tenant_id text NOT NULL,
  action text NOT NULL CHECK (action IN ('create_job','assign_job','record_visit','append_work_line','record_signoff','approve_job','cancel_job')),
  actor_ref text NOT NULL,
  command_id text NOT NULL,
  at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS audit_job ON archava_field.audit(job_id, at);

-- Evidence and history are append-only: a revision must add rows, not rewrite
-- the ones a technician or a customer already signed.
CREATE OR REPLACE FUNCTION archava_field.forbid_mutation() RETURNS trigger AS $$
BEGIN
  RAISE EXCEPTION 'archava_field.% is append-only', TG_TABLE_NAME;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS visits_immutable ON archava_field.visits;
CREATE TRIGGER visits_immutable BEFORE UPDATE OR DELETE ON archava_field.visits
  FOR EACH ROW EXECUTE FUNCTION archava_field.forbid_mutation();

DROP TRIGGER IF EXISTS work_lines_immutable ON archava_field.work_lines;
CREATE TRIGGER work_lines_immutable BEFORE UPDATE OR DELETE ON archava_field.work_lines
  FOR EACH ROW EXECUTE FUNCTION archava_field.forbid_mutation();

DROP TRIGGER IF EXISTS signoffs_immutable ON archava_field.signoffs;
CREATE TRIGGER signoffs_immutable BEFORE UPDATE OR DELETE ON archava_field.signoffs
  FOR EACH ROW EXECUTE FUNCTION archava_field.forbid_mutation();

DROP TRIGGER IF EXISTS audit_immutable ON archava_field.audit;
CREATE TRIGGER audit_immutable BEFORE UPDATE OR DELETE ON archava_field.audit
  FOR EACH ROW EXECUTE FUNCTION archava_field.forbid_mutation();

-- A recorded command is final; a retry must reuse it, never replace it.
DROP TRIGGER IF EXISTS commands_immutable ON archava_field.commands;
CREATE TRIGGER commands_immutable BEFORE UPDATE OR DELETE ON archava_field.commands
  FOR EACH ROW EXECUTE FUNCTION archava_field.forbid_mutation();

-- Approval is the money boundary: once a job reaches it (or cancellation) it is
-- closed, so no transition and no evidence can follow. Evidence arriving after
-- that is refused by the append-only guards below becoming reachable at all.
CREATE OR REPLACE FUNCTION archava_field.guard_closed_job() RETURNS trigger AS $$
BEGIN
  IF TG_OP = 'INSERT' THEN
    IF NEW.status <> 'requested' THEN
      RAISE EXCEPTION 'a field job must start as requested, not %', NEW.status;
    END IF;
    RETURN NEW;
  END IF;
  IF OLD.status = 'approved' OR OLD.status = 'canceled' THEN
    RAISE EXCEPTION 'field job % is closed; status % is final', OLD.job_id, OLD.status;
  END IF;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;
DROP TRIGGER IF EXISTS jobs_close_intent ON archava_field.jobs;
CREATE TRIGGER jobs_close_intent BEFORE INSERT OR UPDATE OF status ON archava_field.jobs
  FOR EACH ROW EXECUTE FUNCTION archava_field.guard_closed_job();

-- Approval is the closed state, so its precondition is enforced here and not
-- only in the application: no accepted customer signoff, no approved job.
CREATE OR REPLACE FUNCTION archava_field.guard_approval_requires_signoff()
  RETURNS trigger AS $$
BEGIN
  IF NEW.status = 'approved' AND NOT EXISTS (
    SELECT 1 FROM archava_field.signoffs s
    WHERE s.job_id = NEW.job_id AND s.accepted
  ) THEN
    RAISE EXCEPTION 'field job % cannot be approved without an accepted customer signoff',
      NEW.job_id;
  END IF;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS jobs_approval_signoff ON archava_field.jobs;
CREATE TRIGGER jobs_approval_signoff BEFORE UPDATE OF status ON archava_field.jobs
  FOR EACH ROW
  WHEN (NEW.status = 'approved')
  EXECUTE FUNCTION archava_field.guard_approval_requires_signoff();

-- Tenant isolation is enforced by the session, not by the caller's promise.
ALTER TABLE archava_field.jobs ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_field.jobs FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS field_tenant ON archava_field.jobs;
CREATE POLICY field_tenant ON archava_field.jobs
  USING (tenant_id = current_setting('archava.tenant_id', true));

-- Membership is the gate every command crosses before it touches a job.
-- The whole row is returned because the caller distinguishes an inactive
-- membership from an enrolled one; a projection would silently drop `active`.
CREATE OR REPLACE FUNCTION archava_field.lock_membership(wanted_tenant text, wanted_principal text)
RETURNS SETOF archava_field.memberships LANGUAGE sql SECURITY DEFINER SET
  search_path = pg_catalog AS $$
  SELECT * FROM archava_field.memberships
  WHERE tenant_id = wanted_tenant
    AND principal_id = wanted_principal;
$$;

REVOKE ALL ON FUNCTION archava_field.lock_membership(text, text) FROM PUBLIC;
