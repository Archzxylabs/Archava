-- Act host 002: the ledger's tenant/session is bound by the connection, not by
-- the caller's argument.
--
-- 001 accepted `tenant_id` and `session_id` as arguments to every ledger
-- function and called `set_config` itself before touching a row. The RLS policy
-- then compared the row against the value the caller had just supplied, so any
-- holder of the runtime role could read or write another tenant's intents by
-- passing that tenant's id. The role also held direct SELECT/INSERT/UPDATE on
-- `heads` and `intents`, which made the policy advisory rather than enforced.
--
-- This migration removes both halves of that hole:
--
--   * every function reads the scope from `archava.tenant_id` /
--     `archava.act_session`, which the host sets from the authenticated session
--     before the call. An unset scope raises instead of matching nothing, so a
--     misconfigured caller fails closed and loudly.
--   * the functions become SECURITY DEFINER with a pinned `search_path`, so the
--     runtime role no longer needs (and no longer receives) table grants. It can
--     only reach the ledger through the scoped functions.
--
-- 001 is deliberately left byte-identical: its checksum is recorded in
-- `schema_migrations` and a changed 001 would refuse every existing install.
-- This file is idempotent and is re-applied on each `migrateActHost` run, so it
-- needs no ledger row of its own.

-- The 001 signatures are replaced, not overloaded: leaving them would keep the
-- self-certifying entry point callable.
DROP FUNCTION IF EXISTS archava_act_host.open_attempt(text, text, text, text, text, bigint, text, bigint, text);
-- @@statement
DROP FUNCTION IF EXISTS archava_act_host.lookup_attempt(text, text, text, text);
-- @@statement
DROP FUNCTION IF EXISTS archava_act_host.find_head(text, text, text, text);
-- @@statement
DROP FUNCTION IF EXISTS archava_act_host.claim_attempt(text, text, text, text);
-- @@statement
DROP FUNCTION IF EXISTS archava_act_host.settle_attempt(text, text, text, text, text);
-- @@statement
DROP FUNCTION IF EXISTS archava_act_host.cancel_attempt(text, text, text);
-- @@statement
DROP FUNCTION IF EXISTS archava_act_host.reconcile_attempt(text, text, text, text);
-- @@statement
DROP FUNCTION IF EXISTS archava_act_host.verify_execution(text, text, text);

-- @@statement
CREATE OR REPLACE FUNCTION archava_act_host.open_attempt(
  wanted_action text, wanted_digest text, candidate text, at_ms bigint,
  ch text, exp_ms bigint, predecessor text
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $fn$
DECLARE
  t text := current_setting('archava.tenant_id', true);
  s text := current_setting('archava.act_session', true);
  current_id text; rec archava_act_host.intents%ROWTYPE;
  mapping archava_act_host.challenges%ROWTYPE;
BEGIN
  IF t IS NULL OR t = '' OR s IS NULL OR s = '' THEN
    RAISE EXCEPTION 'host_scope_unbound';
  END IF;
  INSERT INTO archava_act_host.heads(tenant_id,session_id,action_id,input_digest)
    VALUES(t,s,wanted_action,wanted_digest)
    ON CONFLICT(tenant_id,session_id,action_id,input_digest) DO NOTHING;
  SELECT intent_id INTO current_id FROM archava_act_host.heads
    WHERE tenant_id=t AND session_id=s AND action_id=wanted_action AND input_digest=wanted_digest
    FOR UPDATE;
  IF current_id IS NOT NULL THEN
    SELECT * INTO STRICT rec FROM archava_act_host.intents
      WHERE tenant_id=t AND session_id=s AND intent_id=current_id FOR UPDATE;
  END IF;
  IF predecessor IS NOT NULL THEN
    IF current_id=predecessor AND rec.phase IN ('succeeded','cancelled') THEN
      current_id:=NULL;
    ELSIF current_id IS NULL OR rec.previous_intent_id IS DISTINCT FROM predecessor THEN
      RAISE EXCEPTION 'host_attempt_transition_refused';
    END IF;
  END IF;
  IF current_id IS NULL THEN
    INSERT INTO archava_act_host.intents(tenant_id,session_id,intent_id,action_id,input_digest,occurred_at_ms,generation,previous_intent_id,phase)
      VALUES(t,s,candidate,wanted_action,wanted_digest,GREATEST(at_ms,COALESCE(rec.occurred_at_ms+1,at_ms)),COALESCE(rec.generation+1,1),rec.intent_id,'pending')
      RETURNING * INTO rec;
    UPDATE archava_act_host.heads SET intent_id=rec.intent_id
      WHERE tenant_id=t AND session_id=s AND action_id=wanted_action AND input_digest=wanted_digest;
  END IF;
  INSERT INTO archava_act_host.challenges(tenant_id,session_id,challenge_id,intent_id,expires_at_ms)
    VALUES(t,s,ch,rec.intent_id,exp_ms) ON CONFLICT(challenge_id) DO NOTHING;
  SELECT * INTO mapping FROM archava_act_host.challenges
    WHERE tenant_id=t AND session_id=s AND challenge_id=ch;
  IF mapping.intent_id IS DISTINCT FROM rec.intent_id OR mapping.expires_at_ms IS DISTINCT FROM exp_ms THEN
    RAISE EXCEPTION 'Host mapping mismatch';
  END IF;
  UPDATE archava_act_host.intents SET challenge_id=ch,expires_at_ms=exp_ms
    WHERE tenant_id=t AND session_id=s AND intent_id=rec.intent_id RETURNING * INTO rec;
  RETURN to_jsonb(rec);
END $fn$;

-- @@statement
CREATE OR REPLACE FUNCTION archava_act_host.lookup_attempt(wanted_ref text, kind text)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $fn$
DECLARE
  t text := current_setting('archava.tenant_id', true);
  s text := current_setting('archava.act_session', true);
  rec archava_act_host.intents%ROWTYPE; mapping archava_act_host.challenges%ROWTYPE;
BEGIN
  IF t IS NULL OR t = '' OR s IS NULL OR s = '' THEN
    RAISE EXCEPTION 'host_scope_unbound';
  END IF;
  IF kind='challenge' THEN
    SELECT * INTO mapping FROM archava_act_host.challenges
      WHERE tenant_id=t AND session_id=s AND challenge_id=wanted_ref;
    SELECT * INTO rec FROM archava_act_host.intents
      WHERE tenant_id=t AND session_id=s AND intent_id=mapping.intent_id;
    IF rec.intent_id IS NULL THEN RETURN NULL; END IF;
    RETURN to_jsonb(rec)||jsonb_build_object('challenge_id',mapping.challenge_id,'expires_at_ms',mapping.expires_at_ms);
  ELSIF kind='intent' THEN
    SELECT * INTO rec FROM archava_act_host.intents
      WHERE tenant_id=t AND session_id=s AND intent_id=wanted_ref;
    IF rec.intent_id IS NULL THEN RETURN NULL; END IF;
    RETURN to_jsonb(rec);
  ELSE RAISE EXCEPTION 'Invalid host lookup'; END IF;
END $fn$;

-- @@statement
CREATE OR REPLACE FUNCTION archava_act_host.find_head(wanted_action text, wanted_digest text)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $fn$
DECLARE
  t text := current_setting('archava.tenant_id', true);
  s text := current_setting('archava.act_session', true);
  current_id text;
BEGIN
  IF t IS NULL OR t = '' OR s IS NULL OR s = '' THEN
    RAISE EXCEPTION 'host_scope_unbound';
  END IF;
  SELECT intent_id INTO current_id FROM archava_act_host.heads
    WHERE tenant_id=t AND session_id=s AND action_id=wanted_action AND input_digest=wanted_digest;
  RETURN archava_act_host.lookup_attempt(current_id,'intent');
END $fn$;

-- @@statement
CREATE OR REPLACE FUNCTION archava_act_host.claim_attempt(wanted_ref text, claim text)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $fn$
DECLARE
  t text := current_setting('archava.tenant_id', true);
  s text := current_setting('archava.act_session', true);
  changed integer;
BEGIN
  IF t IS NULL OR t = '' OR s IS NULL OR s = '' THEN
    RAISE EXCEPTION 'host_scope_unbound';
  END IF;
  UPDATE archava_act_host.intents SET phase='executing',claim_id=claim
    WHERE tenant_id=t AND session_id=s AND intent_id=wanted_ref AND phase='pending'
      AND EXISTS(SELECT 1 FROM archava_act_host.challenges
        WHERE tenant_id=t AND session_id=s AND intent_id=wanted_ref AND challenge_id=claim);
  GET DIAGNOSTICS changed=ROW_COUNT; RETURN changed=1;
END $fn$;

-- @@statement
CREATE OR REPLACE FUNCTION archava_act_host.settle_attempt(wanted_ref text, claim text, result text)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $fn$
DECLARE
  t text := current_setting('archava.tenant_id', true);
  s text := current_setting('archava.act_session', true);
  changed integer;
BEGIN
  IF t IS NULL OR t = '' OR s IS NULL OR s = '' THEN
    RAISE EXCEPTION 'host_scope_unbound';
  END IF;
  IF result NOT IN ('succeeded','unknown') THEN RAISE EXCEPTION 'Invalid settlement'; END IF;
  UPDATE archava_act_host.intents SET phase=result
    WHERE tenant_id=t AND session_id=s AND intent_id=wanted_ref AND phase='executing' AND claim_id=claim;
  GET DIAGNOSTICS changed=ROW_COUNT; RETURN changed=1;
END $fn$;

-- @@statement
CREATE OR REPLACE FUNCTION archava_act_host.cancel_attempt(wanted_ref text)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $fn$
DECLARE
  t text := current_setting('archava.tenant_id', true);
  s text := current_setting('archava.act_session', true);
  changed integer;
BEGIN
  IF t IS NULL OR t = '' OR s IS NULL OR s = '' THEN
    RAISE EXCEPTION 'host_scope_unbound';
  END IF;
  UPDATE archava_act_host.intents SET phase='cancelled'
    WHERE tenant_id=t AND session_id=s AND intent_id=wanted_ref AND phase='pending';
  GET DIAGNOSTICS changed=ROW_COUNT; RETURN changed=1;
END $fn$;

-- @@statement
CREATE OR REPLACE FUNCTION archava_act_host.reconcile_attempt(wanted_ref text, proof text)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $fn$
DECLARE
  t text := current_setting('archava.tenant_id', true);
  s text := current_setting('archava.act_session', true);
  changed integer;
BEGIN
  IF t IS NULL OR t = '' OR s IS NULL OR s = '' THEN
    RAISE EXCEPTION 'host_scope_unbound';
  END IF;
  UPDATE archava_act_host.intents SET phase='succeeded',proof_digest=proof
    WHERE tenant_id=t AND session_id=s AND intent_id=wanted_ref AND phase IN ('executing','unknown');
  GET DIAGNOSTICS changed=ROW_COUNT; RETURN changed=1;
END $fn$;

-- The row-lock fence used inside a domain transaction. It is a function rather
-- than an inline SELECT so the runtime role needs no SELECT on `intents`.
-- @@statement
CREATE OR REPLACE FUNCTION archava_act_host.verify_execution(wanted_ref text, claim text, wanted_digest text)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $fn$
DECLARE
  t text := current_setting('archava.tenant_id', true);
  s text := current_setting('archava.act_session', true);
BEGIN
  IF t IS NULL OR t = '' OR s IS NULL OR s = '' THEN
    RAISE EXCEPTION 'host_scope_unbound';
  END IF;
  RETURN EXISTS(
    SELECT 1 FROM archava_act_host.intents i
    WHERE i.tenant_id=t AND i.session_id=s AND i.intent_id=wanted_ref
      AND i.claim_id=claim AND i.input_digest=wanted_digest AND i.phase='executing'
    FOR SHARE OF i
  );
END $fn$;

-- @@statement
REVOKE ALL ON FUNCTION archava_act_host.open_attempt(text,text,text,bigint,text,bigint,text) FROM PUBLIC;
-- @@statement
REVOKE ALL ON FUNCTION archava_act_host.lookup_attempt(text,text) FROM PUBLIC;
-- @@statement
REVOKE ALL ON FUNCTION archava_act_host.find_head(text,text) FROM PUBLIC;
-- @@statement
REVOKE ALL ON FUNCTION archava_act_host.claim_attempt(text,text) FROM PUBLIC;
-- @@statement
REVOKE ALL ON FUNCTION archava_act_host.settle_attempt(text,text,text) FROM PUBLIC;
-- @@statement
REVOKE ALL ON FUNCTION archava_act_host.cancel_attempt(text) FROM PUBLIC;
-- @@statement
REVOKE ALL ON FUNCTION archava_act_host.reconcile_attempt(text,text) FROM PUBLIC;
-- @@statement
REVOKE ALL ON FUNCTION archava_act_host.verify_execution(text,text,text) FROM PUBLIC;
