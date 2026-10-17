-- RA01 — server-side authority records.
--
-- Two additions, both making authority a property of the DATABASE rather than
-- of any request body (audit RA01, 2026-10-16):
--
-- 1. The membership grant gains explicit `role` and `capability` columns. The
--    policy gate has always checked role and capability; before this migration
--    the only place those two facts could come from was the caller's JSON. The
--    defaults are the least-privileged values in each vocabulary
--    (`viewer`/`assist`), so a pre-existing membership row keeps working and
--    can only ever become MORE privileged through an explicit UPDATE.
--
-- 2. Human approvals become bound, single-use rows. The policy gate's
--    `human_approved` boolean was caller-supplied; an approval is now a record
--    that binds the tenant, the actor, the action, the input hash, the
--    revision and an expiry — and, once a DISTINCT human grants it, an
--    approver and an approval instant — after which it can be consumed
--    exactly once. The trigger below enforces the bound-ness at the database
--    level: the host cannot un-approve or un-consume an approval even by bug.
ALTER TABLE archava_commerce.memberships ADD COLUMN role text NOT NULL DEFAULT 'viewer' CHECK(role IN ('archava_assistant','owner','admin','editor','viewer'));
ALTER TABLE archava_commerce.memberships ADD COLUMN capability text NOT NULL DEFAULT 'assist' CHECK(capability IN ('assist','act','transact','enterprise'));
CREATE TABLE archava_commerce.approvals (
 tenant_id text NOT NULL,approval_id uuid NOT NULL,actor_id text NOT NULL,approver_id text,
 action_id text NOT NULL CHECK(length(action_id) BETWEEN 1 AND 120),input_hash text NOT NULL CHECK(length(input_hash)=64),revision integer NOT NULL CHECK(revision>0),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),expires_at timestamptz NOT NULL CHECK(isfinite(expires_at)),
 approved_at timestamptz,consumed_at timestamptz,
 PRIMARY KEY(tenant_id,approval_id),
 FOREIGN KEY(tenant_id,actor_id) REFERENCES archava_commerce.memberships(tenant_id,principal_id),
 FOREIGN KEY(tenant_id,approver_id) REFERENCES archava_commerce.memberships(tenant_id,principal_id),
 CHECK((approved_at IS NULL)=(approver_id IS NULL))
);
CREATE FUNCTION archava_commerce.approvals_are_bound() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 -- The binding fields are immutable once the row exists.
 IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.approval_id IS DISTINCT FROM OLD.approval_id OR NEW.actor_id IS DISTINCT FROM OLD.actor_id OR NEW.action_id IS DISTINCT FROM OLD.action_id OR NEW.input_hash IS DISTINCT FROM OLD.input_hash OR NEW.revision IS DISTINCT FROM OLD.revision OR NEW.created_at IS DISTINCT FROM OLD.created_at OR NEW.expires_at IS DISTINCT FROM OLD.expires_at
 THEN RAISE EXCEPTION 'Commerce approval is bound';END IF;
 -- Approver/approved move NULL → value once, together; never back, never twice.
 IF NEW.approver_id IS DISTINCT FROM OLD.approver_id AND (OLD.approver_id IS NOT NULL OR NEW.approver_id IS NULL OR NEW.approved_at IS NULL)
 THEN RAISE EXCEPTION 'Commerce approval is bound';END IF;
 IF NEW.approved_at IS DISTINCT FROM OLD.approved_at AND (OLD.approved_at IS NOT NULL OR NEW.approved_at IS NULL OR NEW.approver_id IS NULL)
 THEN RAISE EXCEPTION 'Commerce approval is bound';END IF;
 -- Consumption moves NULL → value once, and only on an approved row; never back.
 IF NEW.consumed_at IS DISTINCT FROM OLD.consumed_at AND (OLD.consumed_at IS NOT NULL OR NEW.consumed_at IS NULL OR OLD.approved_at IS NULL)
 THEN RAISE EXCEPTION 'Commerce approval is single-use';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER approval_bound BEFORE UPDATE ON archava_commerce.approvals FOR EACH ROW EXECUTE FUNCTION archava_commerce.approvals_are_bound();
ALTER TABLE archava_commerce.approvals ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_commerce.approvals FORCE ROW LEVEL SECURITY;
CREATE POLICY commerce_tenant ON archava_commerce.approvals USING(tenant_id=current_setting('archava.tenant_id',true)) WITH CHECK(tenant_id=current_setting('archava.tenant_id',true));
