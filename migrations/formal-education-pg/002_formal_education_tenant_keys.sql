-- Formal education 002: the enrollment is the tenant's, not the database's.
--
-- 001 gave `sessions`, `results` and `reviews` their own `tenant_id` and an RLS
-- policy, but the foreign key pointed at `enrollments(enrollment_id)` alone —
-- a globally unique id. A child row in tenant A could therefore name tenant B's
-- enrollment, satisfy both the policy (the child's tenant matches the session)
-- and the FK (the id exists), and quietly attach academic history to another
-- tenant's student. The key becomes composite, so the child's tenant is part of
-- what it references.
--
-- 001 also left `memberships` without RLS while granting it to the runtime
-- role, which let any holder of that role enumerate every tenant's principals,
-- permissions, scopes and teaching classes.

-- The key the children reference. 001's primary key is `enrollment_id` alone.
ALTER TABLE archava_edu.enrollments ADD CONSTRAINT enrollments_tenant_enrollment_key
  UNIQUE (tenant_id, enrollment_id);

-- No row is rewritten here: the children already carry `tenant_id` from 001,
-- so the composite key is added by constraint validation rather than a
-- backfill, and the append-only triggers never fire.

ALTER TABLE archava_edu.sessions DROP CONSTRAINT IF EXISTS sessions_enrollment_id_fkey;
ALTER TABLE archava_edu.sessions
  ADD CONSTRAINT sessions_tenant_enrollment_fkey
  FOREIGN KEY (tenant_id, enrollment_id)
  REFERENCES archava_edu.enrollments(tenant_id, enrollment_id) ON DELETE RESTRICT;

ALTER TABLE archava_edu.results DROP CONSTRAINT IF EXISTS results_enrollment_id_fkey;
ALTER TABLE archava_edu.results
  ADD CONSTRAINT results_tenant_enrollment_fkey
  FOREIGN KEY (tenant_id, enrollment_id)
  REFERENCES archava_edu.enrollments(tenant_id, enrollment_id) ON DELETE RESTRICT;

ALTER TABLE archava_edu.reviews DROP CONSTRAINT IF EXISTS reviews_enrollment_id_fkey;
ALTER TABLE archava_edu.reviews
  ADD CONSTRAINT reviews_tenant_enrollment_fkey
  FOREIGN KEY (tenant_id, enrollment_id)
  REFERENCES archava_edu.enrollments(tenant_id, enrollment_id) ON DELETE RESTRICT;

-- Session, result and review reads all start at the enrollment, and every one
-- of them now filters by tenant first.
CREATE INDEX IF NOT EXISTS edu_sessions_tenant_enrollment
  ON archava_edu.sessions(tenant_id, enrollment_id, recorded_at);
CREATE INDEX IF NOT EXISTS edu_results_tenant_enrollment
  ON archava_edu.results(tenant_id, enrollment_id, recorded_at);
CREATE INDEX IF NOT EXISTS edu_reviews_tenant_enrollment
  ON archava_edu.reviews(tenant_id, enrollment_id, reviewed_at);

-- Membership is the gate every command crosses, so it answers to the session
-- tenant exactly like every other table in this schema.
ALTER TABLE archava_edu.memberships ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_edu.memberships FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS edu_tenant ON archava_edu.memberships;
CREATE POLICY edu_tenant ON archava_edu.memberships
  USING (tenant_id = current_setting('archava.tenant_id', true))
  WITH CHECK (tenant_id = current_setting('archava.tenant_id', true));

-- The lookup is SECURITY DEFINER and exists to resolve the caller's own
-- membership. It must not let the caller name a tenant the session is not
-- already bound to: the argument can narrow the session's scope, never widen it.
CREATE OR REPLACE FUNCTION archava_edu.lock_membership(wanted_tenant text, wanted_principal text)
RETURNS SETOF archava_edu.memberships LANGUAGE sql SECURITY DEFINER SET
  search_path = pg_catalog AS $$
  SELECT * FROM archava_edu.memberships
  WHERE tenant_id = wanted_tenant
    AND wanted_tenant = current_setting('archava.tenant_id', true)
    AND principal_id = wanted_principal;
$$;
REVOKE ALL ON FUNCTION archava_edu.lock_membership(text, text) FROM PUBLIC;

-- `visible_enrollment` reads memberships to evaluate `assigned_class`. With
-- RLS forced, that read is filtered to the session tenant, which is the
-- enrollment's tenant by construction; the tenant predicate is restated on the
-- membership probe so the function never depends on the session alone.
CREATE OR REPLACE FUNCTION archava_edu.visible_enrollment(
  wanted_tenant text, principal_id text, wanted_scope text, enrollment archava_edu.enrollments
) RETURNS boolean LANGUAGE sql SECURITY DEFINER SET search_path = pg_catalog AS $$
  SELECT CASE wanted_scope
    WHEN 'all' THEN true
    WHEN 'guardian' THEN enrollment.guardian_ref = $2
    WHEN 'self' THEN enrollment.student_ref = $2
    WHEN 'assigned_class' THEN EXISTS (
      SELECT 1 FROM archava_edu.memberships m
      WHERE m.tenant_id = $1
        AND m.tenant_id = enrollment.tenant_id
        AND m.principal_id = $2
        AND m.active
        AND m.scope = 'assigned_class'
        AND enrollment.class_ref = ANY(m.classes)
    )
    ELSE false
  END;
$$;
REVOKE ALL ON FUNCTION archava_edu.visible_enrollment(text, text, text, archava_edu.enrollments)
  FROM PUBLIC;
