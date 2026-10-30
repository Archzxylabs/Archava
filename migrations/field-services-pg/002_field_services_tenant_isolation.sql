-- Field services 002: every dispatch child carries its tenant, and the database
-- enforces the isolation that migration 001 only asserted for `jobs`.
--
-- 001 scoped `jobs` with ENABLE/FORCE RLS but left `visits`, `work_lines`,
-- `signoffs`, `audit`, `commands` and `memberships` readable by any role that
-- held the runtime grant. `visits`, `work_lines` and `signoffs` had no
-- `tenant_id` at all, so no policy could be written for them: a child read by
-- `job_id` alone reached whichever tenant had written that job. The tenant
-- column is therefore part of the child's identity, and the foreign key is
-- composite so a row in tenant A can never attach to tenant B's job.

-- The parent key the children point at. 001's primary key is `job_id` alone,
-- which a composite foreign key cannot reference.
ALTER TABLE archava_field.jobs ADD CONSTRAINT jobs_tenant_job_key UNIQUE (tenant_id, job_id);

-- `visits`, `work_lines` and `signoffs` are append-only: their immutability
-- trigger raises on every UPDATE, including the backfill below. The trigger is
-- disabled for the length of this migration only; no row is deleted or changed
-- beyond receiving the tenant its job already has.
ALTER TABLE archava_field.visits DISABLE TRIGGER visits_immutable;
ALTER TABLE archava_field.work_lines DISABLE TRIGGER work_lines_immutable;
ALTER TABLE archava_field.signoffs DISABLE TRIGGER signoffs_immutable;

ALTER TABLE archava_field.visits ADD COLUMN IF NOT EXISTS tenant_id text;
UPDATE archava_field.visits v
   SET tenant_id = j.tenant_id
  FROM archava_field.jobs j
 WHERE j.job_id = v.job_id
   AND v.tenant_id IS NULL;
ALTER TABLE archava_field.visits ALTER COLUMN tenant_id SET NOT NULL;
ALTER TABLE archava_field.visits DROP CONSTRAINT IF EXISTS visits_job_id_fkey;
ALTER TABLE archava_field.visits
  ADD CONSTRAINT visits_tenant_job_fkey
  FOREIGN KEY (tenant_id, job_id)
  REFERENCES archava_field.jobs(tenant_id, job_id) ON DELETE RESTRICT;
CREATE INDEX IF NOT EXISTS visits_tenant_job
  ON archava_field.visits(tenant_id, job_id, visited_at);

ALTER TABLE archava_field.work_lines ADD COLUMN IF NOT EXISTS tenant_id text;
UPDATE archava_field.work_lines l
   SET tenant_id = j.tenant_id
  FROM archava_field.jobs j
 WHERE j.job_id = l.job_id
   AND l.tenant_id IS NULL;
ALTER TABLE archava_field.work_lines ALTER COLUMN tenant_id SET NOT NULL;
ALTER TABLE archava_field.work_lines DROP CONSTRAINT IF EXISTS work_lines_job_id_fkey;
ALTER TABLE archava_field.work_lines
  ADD CONSTRAINT work_lines_tenant_job_fkey
  FOREIGN KEY (tenant_id, job_id)
  REFERENCES archava_field.jobs(tenant_id, job_id) ON DELETE RESTRICT;
CREATE INDEX IF NOT EXISTS work_lines_tenant_job
  ON archava_field.work_lines(tenant_id, job_id, recorded_at);

ALTER TABLE archava_field.signoffs ADD COLUMN IF NOT EXISTS tenant_id text;
UPDATE archava_field.signoffs s
   SET tenant_id = j.tenant_id
  FROM archava_field.jobs j
 WHERE j.job_id = s.job_id
   AND s.tenant_id IS NULL;
ALTER TABLE archava_field.signoffs ALTER COLUMN tenant_id SET NOT NULL;
ALTER TABLE archava_field.signoffs DROP CONSTRAINT IF EXISTS signoffs_job_id_fkey;
ALTER TABLE archava_field.signoffs
  ADD CONSTRAINT signoffs_tenant_job_fkey
  FOREIGN KEY (tenant_id, job_id)
  REFERENCES archava_field.jobs(tenant_id, job_id) ON DELETE RESTRICT;
CREATE INDEX IF NOT EXISTS signoffs_tenant_job
  ON archava_field.signoffs(tenant_id, job_id, signed_at);

ALTER TABLE archava_field.visits ENABLE TRIGGER visits_immutable;
ALTER TABLE archava_field.work_lines ENABLE TRIGGER work_lines_immutable;
ALTER TABLE archava_field.signoffs ENABLE TRIGGER signoffs_immutable;

-- The children, the ledger and the membership table now answer to the session
-- tenant exactly like `jobs`. A policy whose `USING` and `WITH CHECK` are both
-- the tenant predicate fails closed: an unset `archava.tenant_id` matches no
-- row and admits no write.
ALTER TABLE archava_field.memberships ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_field.memberships FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS field_tenant ON archava_field.memberships;
CREATE POLICY field_tenant ON archava_field.memberships
  USING (tenant_id = current_setting('archava.tenant_id', true))
  WITH CHECK (tenant_id = current_setting('archava.tenant_id', true));

ALTER TABLE archava_field.visits ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_field.visits FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS field_tenant ON archava_field.visits;
CREATE POLICY field_tenant ON archava_field.visits
  USING (tenant_id = current_setting('archava.tenant_id', true))
  WITH CHECK (tenant_id = current_setting('archava.tenant_id', true));

ALTER TABLE archava_field.work_lines ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_field.work_lines FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS field_tenant ON archava_field.work_lines;
CREATE POLICY field_tenant ON archava_field.work_lines
  USING (tenant_id = current_setting('archava.tenant_id', true))
  WITH CHECK (tenant_id = current_setting('archava.tenant_id', true));

ALTER TABLE archava_field.signoffs ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_field.signoffs FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS field_tenant ON archava_field.signoffs;
CREATE POLICY field_tenant ON archava_field.signoffs
  USING (tenant_id = current_setting('archava.tenant_id', true))
  WITH CHECK (tenant_id = current_setting('archava.tenant_id', true));

ALTER TABLE archava_field.commands ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_field.commands FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS field_tenant ON archava_field.commands;
CREATE POLICY field_tenant ON archava_field.commands
  USING (tenant_id = current_setting('archava.tenant_id', true))
  WITH CHECK (tenant_id = current_setting('archava.tenant_id', true));

ALTER TABLE archava_field.audit ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_field.audit FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS field_tenant ON archava_field.audit;
CREATE POLICY field_tenant ON archava_field.audit
  USING (tenant_id = current_setting('archava.tenant_id', true))
  WITH CHECK (tenant_id = current_setting('archava.tenant_id', true));

-- The membership lookup is the gate every command crosses. It is SECURITY
-- DEFINER, so it must not let its caller name a tenant the session is not
-- already bound to: the argument can no longer widen the session's scope,
-- only narrow it.
CREATE OR REPLACE FUNCTION archava_field.lock_membership(wanted_tenant text, wanted_principal text)
RETURNS SETOF archava_field.memberships LANGUAGE sql SECURITY DEFINER SET
  search_path = pg_catalog AS $$
  SELECT * FROM archava_field.memberships
  WHERE tenant_id = wanted_tenant
    AND wanted_tenant = current_setting('archava.tenant_id', true)
    AND principal_id = wanted_principal;
$$;
REVOKE ALL ON FUNCTION archava_field.lock_membership(text, text) FROM PUBLIC;

-- Approval reads the signoffs of the job it is approving. The tenant predicate
-- is explicit here as well as enforced by RLS: the approval boundary is the
-- money boundary, so it never relies on a session setting alone.
CREATE OR REPLACE FUNCTION archava_field.guard_approval_requires_signoff()
  RETURNS trigger AS $$
BEGIN
  IF NEW.status = 'approved' AND NOT EXISTS (
    SELECT 1 FROM archava_field.signoffs s
    WHERE s.tenant_id = NEW.tenant_id
      AND s.job_id = NEW.job_id
      AND s.accepted
  ) THEN
    RAISE EXCEPTION 'field job % cannot be approved without an accepted customer signoff',
      NEW.job_id;
  END IF;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;
