-- Durable conversations and reviewed agent goals. No bearer credentials are stored.
CREATE TABLE archava_owner.agent_threads (
 tenant_id text NOT NULL, id uuid NOT NULL, principal_id text NOT NULL,
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,id)
);
CREATE TABLE archava_owner.agent_goals (
 tenant_id text NOT NULL, id uuid NOT NULL, thread_id uuid NOT NULL, principal_id text NOT NULL,
 request_id uuid NOT NULL, parent_id uuid, objective text NOT NULL CHECK(length(objective) BETWEEN 1 AND 2000),
 state text NOT NULL CHECK(state IN ('clarifying','planned','approved','running','holding','completed','failed','cancelled','needs_review','awaiting_session')),
 modules jsonb NOT NULL CHECK(jsonb_typeof(modules)='array'), context jsonb NOT NULL,
 plan jsonb NOT NULL, plan_hash text NOT NULL CHECK(plan_hash ~ '^[0-9a-f]{64}$'),
 approved_hash text, session_hash text CHECK(session_hash IS NULL OR session_hash ~ '^[0-9a-f]{64}$'),
 mission_id uuid, outcome jsonb, failure text,
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(), updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,id), UNIQUE(tenant_id,principal_id,request_id),
 FOREIGN KEY(tenant_id,thread_id) REFERENCES archava_owner.agent_threads(tenant_id,id),
 FOREIGN KEY(tenant_id,parent_id) REFERENCES archava_owner.agent_goals(tenant_id,id),
 CHECK(approved_hash IS NULL OR approved_hash=plan_hash)
);
CREATE INDEX agent_goal_work ON archava_owner.agent_goals(state,updated_at) WHERE state IN ('approved','running');
CREATE TABLE archava_owner.agent_turns (
 tenant_id text NOT NULL, id uuid NOT NULL, thread_id uuid NOT NULL, principal_id text NOT NULL,
 request_id uuid NOT NULL, utterance text NOT NULL CHECK(length(utterance) BETWEEN 1 AND 2000),
 response jsonb NOT NULL, created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,id), UNIQUE(tenant_id,principal_id,request_id),
 FOREIGN KEY(tenant_id,thread_id) REFERENCES archava_owner.agent_threads(tenant_id,id)
);
CREATE TABLE archava_owner.agent_preferences (
 tenant_id text NOT NULL, id uuid NOT NULL, principal_id text NOT NULL, thread_id uuid NOT NULL,
 source_request_id uuid NOT NULL, content text NOT NULL CHECK(length(content) BETWEEN 1 AND 2000),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(), PRIMARY KEY(tenant_id,id),
 UNIQUE(tenant_id,principal_id,source_request_id),
 FOREIGN KEY(tenant_id,thread_id) REFERENCES archava_owner.agent_threads(tenant_id,id)
);
DO $$ DECLARE name text; BEGIN
 FOREACH name IN ARRAY ARRAY['agent_threads','agent_goals','agent_turns','agent_preferences'] LOOP
 EXECUTE format('ALTER TABLE archava_owner.%I ENABLE ROW LEVEL SECURITY',name);
 EXECUTE format('ALTER TABLE archava_owner.%I FORCE ROW LEVEL SECURITY',name);
 EXECUTE format('CREATE POLICY agent_tenant ON archava_owner.%I USING (tenant_id=current_setting(''archava.tenant_id'',true)) WITH CHECK (tenant_id=current_setting(''archava.tenant_id'',true))',name);
 END LOOP;
END $$;
CREATE FUNCTION archava_owner.guard_agent_history() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF TG_OP='DELETE' OR TG_TABLE_NAME<>'agent_goals' THEN RAISE EXCEPTION 'Agent history is append-only'; END IF;
 IF ROW(NEW.tenant_id,NEW.id,NEW.thread_id,NEW.principal_id,NEW.request_id,NEW.parent_id,NEW.objective,NEW.modules,NEW.context,NEW.plan,NEW.plan_hash,NEW.created_at)
 IS DISTINCT FROM ROW(OLD.tenant_id,OLD.id,OLD.thread_id,OLD.principal_id,OLD.request_id,OLD.parent_id,OLD.objective,OLD.modules,OLD.context,OLD.plan,OLD.plan_hash,OLD.created_at)
 THEN RAISE EXCEPTION 'Reviewed goal identity and plan are immutable'; END IF;
 IF OLD.approved_hash IS NOT NULL AND NEW.approved_hash IS DISTINCT FROM OLD.approved_hash THEN RAISE EXCEPTION 'Approval cannot be rebound'; END IF;
 IF OLD.mission_id IS NOT NULL AND NEW.mission_id IS DISTINCT FROM OLD.mission_id THEN RAISE EXCEPTION 'Mission binding is immutable'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER agent_goal_guard BEFORE UPDATE OR DELETE ON archava_owner.agent_goals FOR EACH ROW EXECUTE FUNCTION archava_owner.guard_agent_history();
CREATE TRIGGER agent_thread_guard BEFORE UPDATE OR DELETE ON archava_owner.agent_threads FOR EACH ROW EXECUTE FUNCTION archava_owner.guard_agent_history();
CREATE TRIGGER agent_turn_guard BEFORE UPDATE OR DELETE ON archava_owner.agent_turns FOR EACH ROW EXECUTE FUNCTION archava_owner.guard_agent_history();
CREATE TRIGGER agent_preference_guard BEFORE UPDATE OR DELETE ON archava_owner.agent_preferences FOR EACH ROW EXECUTE FUNCTION archava_owner.guard_agent_history();

-- Routing metadata only. The worker flag grants no access to business/goal contents.
CREATE TABLE archava_owner.agent_work (
 tenant_id text NOT NULL, goal_id uuid NOT NULL, created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,goal_id), FOREIGN KEY(tenant_id,goal_id) REFERENCES archava_owner.agent_goals(tenant_id,id)
);
ALTER TABLE archava_owner.agent_work ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_owner.agent_work FORCE ROW LEVEL SECURITY;
CREATE POLICY agent_work_read ON archava_owner.agent_work FOR SELECT USING (tenant_id=current_setting('archava.tenant_id',true) OR current_setting('archava.agent_worker',true)='on');
CREATE POLICY agent_work_insert ON archava_owner.agent_work FOR INSERT WITH CHECK (tenant_id=current_setting('archava.tenant_id',true));
CREATE POLICY agent_work_delete ON archava_owner.agent_work FOR DELETE USING (tenant_id=current_setting('archava.tenant_id',true));

CREATE TABLE archava_owner.agent_model_attempts (
 tenant_id text NOT NULL, principal_id text NOT NULL, request_id uuid NOT NULL,
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(), PRIMARY KEY(tenant_id,principal_id,request_id)
);
CREATE INDEX agent_model_budget ON archava_owner.agent_model_attempts(tenant_id,principal_id,created_at);
ALTER TABLE archava_owner.agent_model_attempts ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_owner.agent_model_attempts FORCE ROW LEVEL SECURITY;
CREATE POLICY agent_model_tenant ON archava_owner.agent_model_attempts USING (tenant_id=current_setting('archava.tenant_id',true)) WITH CHECK (tenant_id=current_setting('archava.tenant_id',true));
CREATE TRIGGER agent_model_attempt_guard BEFORE UPDATE OR DELETE ON archava_owner.agent_model_attempts FOR EACH ROW EXECUTE FUNCTION archava_owner.guard_agent_history();
