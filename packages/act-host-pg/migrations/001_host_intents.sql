-- Whole installation/re-entry is one atomic statement. The loader substitutes
-- only a computed hex checksum; no caller input appears in this migration.
DO $migration$
DECLARE installed text;
BEGIN
  PERFORM pg_advisory_xact_lock(813406102941);
  IF to_regclass('archava_act_host.schema_migrations') IS NOT NULL THEN
    SELECT checksum INTO installed FROM archava_act_host.schema_migrations WHERE id='001_host_intents';
    IF installed IS DISTINCT FROM '__ARCHAVA_HOST_CHECKSUM__'
       OR (SELECT count(*) FROM archava_act_host.schema_migrations) <> 1 THEN
      RAISE EXCEPTION 'Unsupported host intent migration ledger';
    END IF;
    RETURN;
  END IF;
  CREATE SCHEMA archava_act_host;
  CREATE TABLE archava_act_host.schema_migrations (id text PRIMARY KEY, checksum text NOT NULL);
  CREATE TABLE archava_act_host.intents (
    tenant_id text NOT NULL, session_id text NOT NULL, intent_id text NOT NULL,
    action_id text NOT NULL, input_digest text NOT NULL CHECK(input_digest ~ '^[a-f0-9]{64}$'),
    occurred_at_ms bigint NOT NULL CHECK(occurred_at_ms BETWEEN 0 AND 8640000000000000),
    generation integer NOT NULL CHECK(generation>0), previous_intent_id text,
    phase text NOT NULL CHECK(phase IN ('pending','executing','succeeded','unknown','cancelled')),
    claim_id text, proof_digest text CHECK(proof_digest IS NULL OR proof_digest ~ '^[a-f0-9]{64}$'),
    challenge_id text, expires_at_ms bigint,
    PRIMARY KEY(tenant_id,session_id,intent_id), UNIQUE(intent_id),
    UNIQUE(tenant_id,session_id,action_id,input_digest,generation),
    UNIQUE(tenant_id,session_id,action_id,input_digest,intent_id),
    UNIQUE(tenant_id,session_id,previous_intent_id),
    FOREIGN KEY(tenant_id,session_id,previous_intent_id) REFERENCES archava_act_host.intents(tenant_id,session_id,intent_id),
    CHECK((generation=1)=(previous_intent_id IS NULL)),
    CHECK((phase IN ('pending','cancelled'))=(claim_id IS NULL)),
    CHECK((challenge_id IS NULL)=(expires_at_ms IS NULL)),
    CHECK(expires_at_ms IS NULL OR expires_at_ms>=0)
  );
  CREATE TABLE archava_act_host.heads (
    tenant_id text NOT NULL, session_id text NOT NULL, action_id text NOT NULL,
    input_digest text NOT NULL, intent_id text,
    PRIMARY KEY(tenant_id,session_id,action_id,input_digest),
    FOREIGN KEY(tenant_id,session_id,action_id,input_digest,intent_id)
      REFERENCES archava_act_host.intents(tenant_id,session_id,action_id,input_digest,intent_id)
  );
  CREATE TABLE archava_act_host.challenges (
    tenant_id text NOT NULL, session_id text NOT NULL, challenge_id text NOT NULL,
    intent_id text NOT NULL, expires_at_ms bigint NOT NULL CHECK(expires_at_ms>=0),
    PRIMARY KEY(tenant_id,session_id,challenge_id), UNIQUE(challenge_id),
    UNIQUE(tenant_id,session_id,intent_id,challenge_id),
    FOREIGN KEY(tenant_id,session_id,intent_id) REFERENCES archava_act_host.intents(tenant_id,session_id,intent_id)
  );
  ALTER TABLE archava_act_host.intents ADD CONSTRAINT host_claim_mapping
    FOREIGN KEY(tenant_id,session_id,intent_id,claim_id)
    REFERENCES archava_act_host.challenges(tenant_id,session_id,intent_id,challenge_id);
  ALTER TABLE archava_act_host.intents ADD CONSTRAINT host_latest_mapping
    FOREIGN KEY(tenant_id,session_id,intent_id,challenge_id)
    REFERENCES archava_act_host.challenges(tenant_id,session_id,intent_id,challenge_id);

  CREATE FUNCTION archava_act_host.protect_intent() RETURNS trigger LANGUAGE plpgsql AS $fn$
  BEGIN
    IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Host intents are retained'; END IF;
    IF (NEW.tenant_id,NEW.session_id,NEW.intent_id,NEW.action_id,NEW.input_digest,NEW.occurred_at_ms,NEW.generation,NEW.previous_intent_id)
       IS DISTINCT FROM (OLD.tenant_id,OLD.session_id,OLD.intent_id,OLD.action_id,OLD.input_digest,OLD.occurred_at_ms,OLD.generation,OLD.previous_intent_id)
       THEN RAISE EXCEPTION 'Host intent identity is immutable'; END IF;
    IF NEW.phase=OLD.phase THEN
      IF (NEW.claim_id,NEW.proof_digest) IS DISTINCT FROM (OLD.claim_id,OLD.proof_digest)
        THEN RAISE EXCEPTION 'Host settlement evidence is immutable'; END IF;
    ELSIF OLD.phase='pending' AND NEW.phase IN ('executing','cancelled') THEN
      IF NEW.proof_digest IS NOT NULL THEN RAISE EXCEPTION 'No outcome is known yet'; END IF;
    ELSIF OLD.phase='executing' AND NEW.phase IN ('succeeded','unknown') THEN
      IF NEW.claim_id IS DISTINCT FROM OLD.claim_id THEN RAISE EXCEPTION 'Claim identity is immutable'; END IF;
    ELSIF OLD.phase='unknown' AND NEW.phase='succeeded' AND NEW.proof_digest IS NOT NULL THEN
      IF NEW.claim_id IS DISTINCT FROM OLD.claim_id THEN RAISE EXCEPTION 'Claim identity is immutable'; END IF;
    ELSE RAISE EXCEPTION 'Unsafe host phase transition'; END IF;
    RETURN NEW;
  END $fn$;
  CREATE TRIGGER host_intent_guard BEFORE UPDATE OR DELETE ON archava_act_host.intents
    FOR EACH ROW EXECUTE FUNCTION archava_act_host.protect_intent();
  CREATE FUNCTION archava_act_host.protect_mapping() RETURNS trigger LANGUAGE plpgsql AS $fn$
  BEGIN RAISE EXCEPTION 'Host challenge mappings are immutable'; END $fn$;
  CREATE TRIGGER host_mapping_guard BEFORE UPDATE OR DELETE ON archava_act_host.challenges
    FOR EACH ROW EXECUTE FUNCTION archava_act_host.protect_mapping();
  ALTER TABLE archava_act_host.intents ENABLE ROW LEVEL SECURITY;
  ALTER TABLE archava_act_host.intents FORCE ROW LEVEL SECURITY;
  ALTER TABLE archava_act_host.heads ENABLE ROW LEVEL SECURITY;
  ALTER TABLE archava_act_host.heads FORCE ROW LEVEL SECURITY;
  ALTER TABLE archava_act_host.challenges ENABLE ROW LEVEL SECURITY;
  ALTER TABLE archava_act_host.challenges FORCE ROW LEVEL SECURITY;
  CREATE POLICY host_intent_scope ON archava_act_host.intents USING(tenant_id=current_setting('archava.tenant_id',true) AND session_id=current_setting('archava.act_session',true)) WITH CHECK(tenant_id=current_setting('archava.tenant_id',true) AND session_id=current_setting('archava.act_session',true));
  CREATE POLICY host_head_scope ON archava_act_host.heads USING(tenant_id=current_setting('archava.tenant_id',true) AND session_id=current_setting('archava.act_session',true)) WITH CHECK(tenant_id=current_setting('archava.tenant_id',true) AND session_id=current_setting('archava.act_session',true));
  CREATE POLICY host_challenge_scope ON archava_act_host.challenges USING(tenant_id=current_setting('archava.tenant_id',true) AND session_id=current_setting('archava.act_session',true)) WITH CHECK(tenant_id=current_setting('archava.tenant_id',true) AND session_id=current_setting('archava.act_session',true));

  CREATE FUNCTION archava_act_host.open_attempt(t text,s text,a text,d text,candidate text,at_ms bigint,ch text,exp_ms bigint,predecessor text)
    RETURNS jsonb LANGUAGE plpgsql SECURITY INVOKER AS $fn$
  DECLARE current_id text; rec archava_act_host.intents%ROWTYPE; mapping archava_act_host.challenges%ROWTYPE;
  BEGIN
    PERFORM set_config('archava.tenant_id',t,true),set_config('archava.act_session',s,true);
    INSERT INTO archava_act_host.heads(tenant_id,session_id,action_id,input_digest) VALUES(t,s,a,d) ON CONFLICT(tenant_id,session_id,action_id,input_digest) DO NOTHING;
    SELECT intent_id INTO current_id FROM archava_act_host.heads WHERE tenant_id=t AND session_id=s AND action_id=a AND input_digest=d FOR UPDATE;
    IF current_id IS NOT NULL THEN
      SELECT * INTO STRICT rec FROM archava_act_host.intents WHERE tenant_id=t AND session_id=s AND intent_id=current_id FOR UPDATE;
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
        VALUES(t,s,candidate,a,d,GREATEST(at_ms,COALESCE(rec.occurred_at_ms+1,at_ms)),COALESCE(rec.generation+1,1),rec.intent_id,'pending') RETURNING * INTO rec;
      UPDATE archava_act_host.heads SET intent_id=rec.intent_id WHERE tenant_id=t AND session_id=s AND action_id=a AND input_digest=d;
    END IF;
    INSERT INTO archava_act_host.challenges(tenant_id,session_id,challenge_id,intent_id,expires_at_ms) VALUES(t,s,ch,rec.intent_id,exp_ms) ON CONFLICT(challenge_id) DO NOTHING;
    SELECT * INTO mapping FROM archava_act_host.challenges WHERE tenant_id=t AND session_id=s AND challenge_id=ch;
    IF mapping.intent_id IS DISTINCT FROM rec.intent_id OR mapping.expires_at_ms IS DISTINCT FROM exp_ms THEN RAISE EXCEPTION 'Host mapping mismatch'; END IF;
    UPDATE archava_act_host.intents SET challenge_id=ch,expires_at_ms=exp_ms WHERE tenant_id=t AND session_id=s AND intent_id=rec.intent_id RETURNING * INTO rec;
    RETURN to_jsonb(rec);
  END $fn$;

  CREATE FUNCTION archava_act_host.lookup_attempt(t text,s text,ref text,kind text) RETURNS jsonb LANGUAGE plpgsql SECURITY INVOKER AS $fn$
  DECLARE rec archava_act_host.intents%ROWTYPE; mapping archava_act_host.challenges%ROWTYPE;
  BEGIN
    PERFORM set_config('archava.tenant_id',t,true),set_config('archava.act_session',s,true);
    IF kind='challenge' THEN
      SELECT * INTO mapping FROM archava_act_host.challenges WHERE tenant_id=t AND session_id=s AND challenge_id=ref;
      SELECT * INTO rec FROM archava_act_host.intents WHERE tenant_id=t AND session_id=s AND intent_id=mapping.intent_id;
      IF rec.intent_id IS NULL THEN RETURN NULL; END IF;
      RETURN to_jsonb(rec)||jsonb_build_object('challenge_id',mapping.challenge_id,'expires_at_ms',mapping.expires_at_ms);
    ELSIF kind='intent' THEN
      SELECT * INTO rec FROM archava_act_host.intents WHERE tenant_id=t AND session_id=s AND intent_id=ref;
      IF rec.intent_id IS NULL THEN RETURN NULL; END IF;
      RETURN to_jsonb(rec);
    ELSE RAISE EXCEPTION 'Invalid host lookup'; END IF;
  END $fn$;
  CREATE FUNCTION archava_act_host.find_head(t text,s text,a text,d text) RETURNS jsonb LANGUAGE plpgsql SECURITY INVOKER AS $fn$
  DECLARE current_id text;
  BEGIN
    PERFORM set_config('archava.tenant_id',t,true),set_config('archava.act_session',s,true);
    SELECT intent_id INTO current_id FROM archava_act_host.heads WHERE tenant_id=t AND session_id=s AND action_id=a AND input_digest=d;
    RETURN archava_act_host.lookup_attempt(t,s,current_id,'intent');
  END $fn$;

  CREATE FUNCTION archava_act_host.claim_attempt(t text,s text,ref text,claim text) RETURNS boolean LANGUAGE plpgsql SECURITY INVOKER AS $fn$
  DECLARE changed integer;
  BEGIN
    PERFORM set_config('archava.tenant_id',t,true),set_config('archava.act_session',s,true);
    UPDATE archava_act_host.intents SET phase='executing',claim_id=claim WHERE tenant_id=t AND session_id=s AND intent_id=ref AND phase='pending'
      AND EXISTS(SELECT 1 FROM archava_act_host.challenges WHERE tenant_id=t AND session_id=s AND intent_id=ref AND challenge_id=claim);
    GET DIAGNOSTICS changed=ROW_COUNT; RETURN changed=1;
  END $fn$;
  CREATE FUNCTION archava_act_host.settle_attempt(t text,s text,ref text,claim text,result text) RETURNS boolean LANGUAGE plpgsql SECURITY INVOKER AS $fn$
  DECLARE changed integer;
  BEGIN
    PERFORM set_config('archava.tenant_id',t,true),set_config('archava.act_session',s,true);
    IF result NOT IN ('succeeded','unknown') THEN RAISE EXCEPTION 'Invalid settlement'; END IF;
    UPDATE archava_act_host.intents SET phase=result WHERE tenant_id=t AND session_id=s AND intent_id=ref AND phase='executing' AND claim_id=claim;
    GET DIAGNOSTICS changed=ROW_COUNT; RETURN changed=1;
  END $fn$;
  CREATE FUNCTION archava_act_host.cancel_attempt(t text,s text,ref text) RETURNS boolean LANGUAGE plpgsql SECURITY INVOKER AS $fn$
  DECLARE changed integer;
  BEGIN
    PERFORM set_config('archava.tenant_id',t,true),set_config('archava.act_session',s,true);
    UPDATE archava_act_host.intents SET phase='cancelled' WHERE tenant_id=t AND session_id=s AND intent_id=ref AND phase='pending';
    GET DIAGNOSTICS changed=ROW_COUNT; RETURN changed=1;
  END $fn$;
  CREATE FUNCTION archava_act_host.reconcile_attempt(t text,s text,ref text,proof text) RETURNS boolean LANGUAGE plpgsql SECURITY INVOKER AS $fn$
  DECLARE changed integer;
  BEGIN
    PERFORM set_config('archava.tenant_id',t,true),set_config('archava.act_session',s,true);
    UPDATE archava_act_host.intents SET phase='succeeded',proof_digest=proof WHERE tenant_id=t AND session_id=s AND intent_id=ref AND phase IN ('executing','unknown');
    GET DIAGNOSTICS changed=ROW_COUNT; RETURN changed=1;
  END $fn$;
  REVOKE ALL ON ALL TABLES IN SCHEMA archava_act_host FROM PUBLIC;
  REVOKE ALL ON ALL FUNCTIONS IN SCHEMA archava_act_host FROM PUBLIC;
  INSERT INTO archava_act_host.schema_migrations VALUES('001_host_intents','__ARCHAVA_HOST_CHECKSUM__');
END $migration$;
