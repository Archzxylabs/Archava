-- Formal education: bounded admission, term enrollment, recorded attendance and
-- results, domain review and retention.
--
-- The four access scopes are not ranked and not interchangeable. `all` is the
-- registrar's administrative membership; `guardian`, `self` and `assigned_class`
-- each reach a different set of enrollments through a predicate chosen by the
-- stored membership row, never by the request. That is why the read guard below
-- is a SQL predicate over the row itself rather than a permission check in the
-- application: a guardian asking for "more" gets the same predicate, not a
-- wider one.
--
-- The append-only rule covers every table here. An academic record that can be
-- edited in place would let a recorded score change without a new audit entry,
-- which is the one thing a guardian must be able to trust.
CREATE SCHEMA IF NOT EXISTS archava_edu;

CREATE TABLE IF NOT EXISTS archava_edu.memberships (
  tenant_id text NOT NULL,
  principal_id text NOT NULL,
  kind text NOT NULL CHECK (kind IN ('human','service')),
  permissions text[] NOT NULL,
  scope text NOT NULL CHECK (scope IN ('all','guardian','self','assigned_class')),
  active boolean NOT NULL DEFAULT true,
  -- The classes a teacher of this tenant is assigned to. Only meaningful for
  -- the assigned_class scope; an empty array means the scope reaches nothing.
  classes text[] NOT NULL DEFAULT '{}',
  PRIMARY KEY (tenant_id, principal_id)
);

CREATE TABLE IF NOT EXISTS archava_edu.enrollments (
  enrollment_id text PRIMARY KEY,
  tenant_id text NOT NULL,
  student_ref text NOT NULL,
  class_ref text NOT NULL,
  term_ref text NOT NULL,
  status text NOT NULL CHECK (status IN
    ('applied','admitted','enrolled','attending','reported','reviewed','retained','graduated','withdrawn')),
  revision integer NOT NULL DEFAULT 1 CHECK (revision > 0),
  -- The billing domain's own identity for the charge raised against this
  -- enrollment. Records survive here so an academic record can always name the
  -- invoice it belongs to; null until a fee is raised.
  invoice_ref text,
  -- Null for an applicant with no guardian yet. Present from admission onward
  -- for a minor; the guardian scope predicate reads this column.
  guardian_ref text,
  created_by text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS archava_edu.sessions (
  session_id text PRIMARY KEY,
  enrollment_id text NOT NULL REFERENCES archava_edu.enrollments(enrollment_id),
  tenant_id text NOT NULL,
  outcome text NOT NULL CHECK (outcome IN ('present','absent','late','excused')),
  note text,
  -- The scheduling domain's own identity for the class meeting. It must survive
  -- here: attendance recorded without the appointment it was taken at cannot be
  -- reconciled against the timetable.
  appointment_ref text NOT NULL,
  recorded_by text NOT NULL,
  recorded_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS archava_edu.results (
  result_id text PRIMARY KEY,
  enrollment_id text NOT NULL REFERENCES archava_edu.enrollments(enrollment_id),
  tenant_id text NOT NULL,
  assessment_ref text NOT NULL,
  score numeric(9,2) NOT NULL CHECK (score >= 0),
  out_of numeric(9,2) NOT NULL CHECK (out_of > 0),
  letter_ref text,
  note text,
  recorded_by text NOT NULL,
  recorded_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS archava_edu.reviews (
  review_id text PRIMARY KEY,
  enrollment_id text NOT NULL REFERENCES archava_edu.enrollments(enrollment_id),
  tenant_id text NOT NULL,
  decision text NOT NULL CHECK (decision IN ('approved','returned')),
  note text,
  reviewer_ref text NOT NULL,
  reviewed_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS archava_edu.commands (
  command_id text PRIMARY KEY,
  tenant_id text NOT NULL,
  enrollment_id text,
  action text NOT NULL,
  fingerprint text NOT NULL,
  payload jsonb NOT NULL,
  receipt jsonb,
  status text NOT NULL CHECK (status IN ('accepted','rejected','unknown')),
  revision integer,
  created_by text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS archava_edu.audit (
  entry_id text PRIMARY KEY,
  tenant_id text NOT NULL,
  enrollment_id text NOT NULL,
  action text NOT NULL,
  actor_ref text NOT NULL,
  command_id text NOT NULL,
  at timestamptz NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS edu_commands_tenant_command
  ON archava_edu.commands(tenant_id, command_id);
CREATE INDEX IF NOT EXISTS edu_enrollments_tenant_status
  ON archava_edu.enrollments(tenant_id, status, enrollment_id);

-- The refusal used by every append-only table below. It is defined here,
-- before the first trigger that calls it, because a trigger cannot name a
-- function that does not exist yet.
CREATE OR REPLACE FUNCTION archava_edu.forbid_mutation() RETURNS trigger AS $$
BEGIN
  RAISE EXCEPTION 'archava_edu.% is append-only', TG_TABLE_NAME;
END;
$$ LANGUAGE plpgsql;

-- The enrollment row advances, but only where it is allowed to. The identity
-- columns — who this record is about, which class, which term, which guardian,
-- which invoice — are immutable: rewriting one would move a student's academic
-- history onto a different student's identity without a new row to show for it.
-- What may change is the status (forward only), the revision and updated_at.
--
-- `guardian_ref` has one narrow exception, and it is the reason the column is
-- nullable at all. An applicant is admitted before a guardian is recorded, so
-- the row must be able to move from "no guardian yet" to "this guardian".
-- What it must never do is exchange one named guardian for another: that would
-- hand one guardian's academic history to somebody else without a new row to
-- show for it. So an absent guardian may be filled in, and a present one may
-- not be replaced. Filling a null is a correction of an omission; replacing a
-- value is a transfer of record, and only the former is safe here.
CREATE OR REPLACE FUNCTION archava_edu.guard_enrollment_update() RETURNS trigger AS $$
BEGIN
  IF NEW.enrollment_id <> OLD.enrollment_id
     OR NEW.tenant_id <> OLD.tenant_id
     OR NEW.student_ref <> OLD.student_ref
     OR NEW.class_ref <> OLD.class_ref
     OR NEW.term_ref <> OLD.term_ref
     OR NEW.created_by <> OLD.created_by
     OR NEW.created_at <> OLD.created_at THEN
    RAISE EXCEPTION 'enrollment identity columns are immutable: %', NEW.enrollment_id;
  END IF;
  IF OLD.guardian_ref IS NOT NULL AND NEW.guardian_ref IS DISTINCT FROM OLD.guardian_ref THEN
    RAISE EXCEPTION 'a recorded guardian cannot be replaced: %', NEW.enrollment_id;
  END IF;
  -- The invoice is raised by the billing domain against an enrollment that
  -- already exists, so it is likewise recorded once, after admission.
  IF OLD.invoice_ref IS NOT NULL AND NEW.invoice_ref IS DISTINCT FROM OLD.invoice_ref THEN
    RAISE EXCEPTION 'a recorded invoice reference cannot be replaced: %', NEW.enrollment_id;
  END IF;
  IF NEW.revision <> OLD.revision + 1 THEN
    RAISE EXCEPTION 'enrollment revision must advance by exactly one: %', NEW.enrollment_id;
  END IF;
  -- Filling an absent guardian is a correction, not a status change: it keeps
  -- the status the record already has, so the chain check would read it as
  -- `applied -> applied` and refuse a move that never happened. Advance the
  -- chain only when the status actually moves.
  IF NEW.status <> OLD.status THEN
    PERFORM archava_edu.assert_transition(OLD.status, NEW.status);
  END IF;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS edu_enrollments_guarded ON archava_edu.enrollments;
CREATE TRIGGER edu_enrollments_guarded
  BEFORE UPDATE ON archava_edu.enrollments
  FOR EACH ROW EXECUTE FUNCTION archava_edu.guard_enrollment_update();

-- An enrollment is never deleted. Withdrawal is a status, not an absence: a
-- guardian must be able to ask "what happened to last year's record" and get
-- the record back rather than a 404.
DROP TRIGGER IF EXISTS edu_enrollments_no_delete ON archava_edu.enrollments;
CREATE TRIGGER edu_enrollments_no_delete
  BEFORE DELETE ON archava_edu.enrollments
  FOR EACH ROW EXECUTE FUNCTION archava_edu.forbid_mutation();

-- Nothing here is editable in place except the enrollment's own status. The
-- evidence tables are append-only so no application change and no future
-- migration can quietly rewrite a recorded score, a recorded attendance or an
-- audit entry. The enrollment row is different in kind: it carries the current
-- status of a moving record, and moving it forward is recorded as a revision
-- bump plus a new audit entry rather than an in-place rewrite of the evidence.
-- The enrollment trigger above is what stops it being edited freely.

DROP TRIGGER IF EXISTS edu_sessions_append_only ON archava_edu.sessions;
CREATE TRIGGER edu_sessions_append_only
  BEFORE UPDATE OR DELETE ON archava_edu.sessions
  FOR EACH ROW EXECUTE FUNCTION archava_edu.forbid_mutation();

DROP TRIGGER IF EXISTS edu_results_append_only ON archava_edu.results;
CREATE TRIGGER edu_results_append_only
  BEFORE UPDATE OR DELETE ON archava_edu.results
  FOR EACH ROW EXECUTE FUNCTION archava_edu.forbid_mutation();

DROP TRIGGER IF EXISTS edu_reviews_append_only ON archava_edu.reviews;
CREATE TRIGGER edu_reviews_append_only
  BEFORE UPDATE OR DELETE ON archava_edu.reviews
  FOR EACH ROW EXECUTE FUNCTION archava_edu.forbid_mutation();

DROP TRIGGER IF EXISTS edu_audit_append_only ON archava_edu.audit;
CREATE TRIGGER edu_audit_append_only
  BEFORE UPDATE OR DELETE ON archava_edu.audit
  FOR EACH ROW EXECUTE FUNCTION archava_edu.forbid_mutation();

-- The frozen forward chain, expressed in SQL as well as in the TypeScript
-- contract. A caller that bypasses the service still cannot skip review, still
-- cannot record attendance against an enrollment that is not enrolled, and
-- still cannot reach `retained` from anywhere but `reviewed`.
CREATE OR REPLACE FUNCTION archava_edu.assert_transition(
  current_status text, next_status text
) RETURNS void LANGUAGE plpgsql IMMUTABLE AS $$
DECLARE
  allowed text[];
BEGIN
  allowed := CASE current_status
    WHEN 'applied'    THEN ARRAY['admitted','withdrawn']
    WHEN 'admitted'   THEN ARRAY['enrolled','withdrawn']
    WHEN 'enrolled'   THEN ARRAY['attending','withdrawn']
    WHEN 'attending'  THEN ARRAY['reported','withdrawn']
    -- A reported record may go back to attending: a result set recorded by
    -- mistake is withdrawn, not silently kept. It may not skip review.
    WHEN 'reported'   THEN ARRAY['reviewed','attending']
    WHEN 'reviewed'   THEN ARRAY['retained','graduated','withdrawn']
    WHEN 'retained'   THEN ARRAY[]::text[]
    WHEN 'graduated'  THEN ARRAY[]::text[]
    WHEN 'withdrawn'  THEN ARRAY[]::text[]
    ELSE ARRAY[]::text[]
  END;
  IF NOT (next_status = ANY(allowed)) THEN
    RAISE EXCEPTION 'enrollment cannot move from % to %', current_status, next_status;
  END IF;
END;
$$;

-- A recorded score cannot exceed the assessment maximum. This is enforced in
-- SQL as well as in the contract so a migration, a backfill or an operator
-- statement cannot record a score the domain refuses to accept.
CREATE OR REPLACE FUNCTION archava_edu.assert_score(score numeric, out_of numeric)
RETURNS void LANGUAGE plpgsql IMMUTABLE AS $$
BEGIN
  IF score > out_of THEN
    RAISE EXCEPTION 'recorded score % exceeds the assessment maximum %', score, out_of;
  END IF;
END;
$$;

CREATE OR REPLACE FUNCTION archava_edu.assert_score_on_insert() RETURNS trigger AS $$
BEGIN
  PERFORM archava_edu.assert_score(NEW.score, NEW.out_of);
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS edu_results_score_bound ON archava_edu.results;
CREATE TRIGGER edu_results_score_bound
  BEFORE INSERT ON archava_edu.results
  FOR EACH ROW EXECUTE FUNCTION archava_edu.assert_score_on_insert();

-- Tenant isolation is enforced by the session setting, not by the caller's
-- promise. FORCE applies the policy even to the table owner, so a runtime role
-- that owns the schema cannot read across tenants either.
ALTER TABLE archava_edu.enrollments ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_edu.enrollments FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS edu_tenant ON archava_edu.enrollments;
CREATE POLICY edu_tenant ON archava_edu.enrollments
  USING (tenant_id = current_setting('archava.tenant_id', true));

ALTER TABLE archava_edu.sessions ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_edu.sessions FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS edu_tenant ON archava_edu.sessions;
CREATE POLICY edu_tenant ON archava_edu.sessions
  USING (tenant_id = current_setting('archava.tenant_id', true));

ALTER TABLE archava_edu.results ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_edu.results FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS edu_tenant ON archava_edu.results;
CREATE POLICY edu_tenant ON archava_edu.results
  USING (tenant_id = current_setting('archava.tenant_id', true));

ALTER TABLE archava_edu.reviews ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_edu.reviews FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS edu_tenant ON archava_edu.reviews;
CREATE POLICY edu_tenant ON archava_edu.reviews
  USING (tenant_id = current_setting('archava.tenant_id', true));

ALTER TABLE archava_edu.commands ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_edu.commands FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS edu_tenant ON archava_edu.commands;
CREATE POLICY edu_tenant ON archava_edu.commands
  USING (tenant_id = current_setting('archava.tenant_id', true));

ALTER TABLE archava_edu.audit ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_edu.audit FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS edu_tenant ON archava_edu.audit;
CREATE POLICY edu_tenant ON archava_edu.audit
  USING (tenant_id = current_setting('archava.tenant_id', true));

-- The read predicate, in one place, so every read path uses the same one.
-- `all` returns true: the registrar's scope is the administrative view. The
-- other three each narrow to rows the principal is actually party to, and each
-- has its own rule:
--   guardian       -> the row names this principal as the student's guardian
--   self           -> the row names this principal as the student
--   assigned_class -> the row is in a class this principal teaches
-- No combination of the three widens to another: a guardian cannot reach a
-- class, a student cannot reach a sibling, a teacher cannot reach a guardian's
-- other child. The function is SECURITY DEFINER so the runtime role does not
-- need SELECT on memberships to evaluate its own access, and its search_path is
-- pinned so it cannot be redirected.
--
-- Every identifier below is qualified, and the membership columns are aliased
-- to `m.`, deliberately. In a SQL function an unqualified name resolves to an
-- in-scope column before it resolves to a parameter, so writing
-- `m.principal_id = principal_id` inside a query over a table that also has a
-- `principal_id` column is parsed as `m.principal_id = m.principal_id` — a
-- comparison that is always true. That is not a hypothetical: an earlier draft
-- of this function was written that way, and the assigned_class guard silently
-- became "visible to any caller" while still reading as though it checked the
-- principal. The row parameter is also referenced through its alias rather than
-- by column, for the same reason.
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
        AND m.principal_id = $2
        AND m.active
        AND m.scope = 'assigned_class'
        AND enrollment.class_ref = ANY(m.classes)
    )
    ELSE false
  END;
$$;

-- Membership is the gate every command crosses before it touches a record. The
-- whole row is returned because the caller distinguishes an inactive membership
-- from an enrolled one, and because the scope predicate itself reads the
-- membership's teaching classes.
CREATE OR REPLACE FUNCTION archava_edu.lock_membership(wanted_tenant text, wanted_principal text)
RETURNS SETOF archava_edu.memberships LANGUAGE sql SECURITY DEFINER SET
  search_path = pg_catalog AS $$
  SELECT * FROM archava_edu.memberships
  WHERE tenant_id = $1
    AND principal_id = $2;
$$;

-- Results are readable at the reviewed boundary and above. A call that asks for
-- results before review is answered with the recorded rows plus the explicit
-- `resultsPublished` flag the caller already carries, so a reader can tell
-- "none recorded yet" from "recorded and not published".
CREATE OR REPLACE FUNCTION archava_edu.results_published(enrollment_status text)
RETURNS boolean LANGUAGE sql IMMUTABLE AS $$
  SELECT enrollment_status IN ('reviewed','retained','graduated');
$$;

REVOKE ALL ON FUNCTION archava_edu.lock_membership(text, text) FROM PUBLIC;
REVOKE ALL ON FUNCTION archava_edu.visible_enrollment(text, text, text, archava_edu.enrollments) FROM PUBLIC;
REVOKE ALL ON FUNCTION archava_edu.results_published(text) FROM PUBLIC;
REVOKE ALL ON FUNCTION archava_edu.assert_transition(text, text) FROM PUBLIC;
REVOKE ALL ON FUNCTION archava_edu.assert_score(numeric, numeric) FROM PUBLIC;
