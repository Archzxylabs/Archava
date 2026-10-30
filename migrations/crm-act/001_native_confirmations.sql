CREATE SCHEMA archava_crm_act;
CREATE TABLE archava_crm_act.schema_migrations(id text PRIMARY KEY,checksum text NOT NULL);
CREATE TABLE archava_crm_act.confirmation_challenges (
 id text PRIMARY KEY, tenant_id text NOT NULL, session_id text NOT NULL, action_id text NOT NULL,
 token_digest text NOT NULL CHECK(token_digest ~ '^[a-f0-9]{64}$'),
 binding_digest text NOT NULL CHECK(binding_digest ~ '^[a-f0-9]{64}$'),
 issued_at bigint NOT NULL CHECK(issued_at>=0), expires_at bigint NOT NULL CHECK(expires_at>issued_at),
 state text NOT NULL DEFAULT 'pending' CHECK(state IN ('pending','consumed','expired','revoked')),
 consumed_at bigint,
 CHECK((state='consumed')=(consumed_at IS NOT NULL)),
 CHECK(consumed_at IS NULL OR (consumed_at>=issued_at AND consumed_at<expires_at))
);
ALTER TABLE archava_crm_act.confirmation_challenges ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_crm_act.confirmation_challenges FORCE ROW LEVEL SECURITY;
CREATE POLICY native_confirmation_scope ON archava_crm_act.confirmation_challenges
 USING(tenant_id=current_setting('archava.tenant_id',true) AND session_id=current_setting('archava.act_session',true))
 WITH CHECK(tenant_id=current_setting('archava.tenant_id',true) AND session_id=current_setting('archava.act_session',true));
CREATE FUNCTION archava_crm_act.protect_confirmation() RETURNS trigger LANGUAGE plpgsql AS $guard$
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Native confirmations are retained'; END IF;
 IF (NEW.id,NEW.tenant_id,NEW.session_id,NEW.action_id,NEW.token_digest,NEW.binding_digest,NEW.issued_at,NEW.expires_at)
  IS DISTINCT FROM (OLD.id,OLD.tenant_id,OLD.session_id,OLD.action_id,OLD.token_digest,OLD.binding_digest,OLD.issued_at,OLD.expires_at)
  OR OLD.state<>'pending' OR NEW.state NOT IN ('consumed','expired','revoked') THEN
  RAISE EXCEPTION 'Native confirmation transition refused';
 END IF;
 RETURN NEW;
END $guard$;
CREATE TRIGGER native_confirmation_guard BEFORE UPDATE OR DELETE ON archava_crm_act.confirmation_challenges
 FOR EACH ROW EXECUTE FUNCTION archava_crm_act.protect_confirmation();
REVOKE ALL ON ALL TABLES IN SCHEMA archava_crm_act FROM PUBLIC;
REVOKE ALL ON ALL FUNCTIONS IN SCHEMA archava_crm_act FROM PUBLIC;
