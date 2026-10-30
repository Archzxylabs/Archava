CREATE TABLE archava_support.memberships (
 tenant_id text NOT NULL,principal_id text NOT NULL,display_name text NOT NULL DEFAULT '' CHECK(length(display_name)<=254),kind text NOT NULL CHECK(kind IN ('human','service')),permissions text[] NOT NULL CHECK(permissions <@ ARRAY['cases.read','cases.write','cases.assign','notes.read','notes.write','audit.read']::text[]),scope text NOT NULL CHECK(scope IN ('all','assigned')),active boolean NOT NULL DEFAULT true,PRIMARY KEY(tenant_id,principal_id)
);
CREATE TABLE archava_support.cases (
 tenant_id text NOT NULL,id uuid NOT NULL,owner_id text NOT NULL,assignee_id text,requester_name text NOT NULL CHECK(length(requester_name) BETWEEN 1 AND 160),subject text NOT NULL CHECK(length(subject) BETWEEN 1 AND 160),description text NOT NULL CHECK(length(description) BETWEEN 1 AND 4000),priority text NOT NULL CHECK(priority IN ('low','normal','high')),state text NOT NULL DEFAULT 'open' CHECK(state IN ('open','in_progress','waiting','resolved','closed','cancelled')),due_at timestamptz,cycle integer NOT NULL DEFAULT 1 CHECK(cycle>0),resolution text CHECK(length(resolution) BETWEEN 1 AND 4000),resolved_at timestamptz,closed_at timestamptz,
 revision integer NOT NULL DEFAULT 1 CHECK(revision>0),created_at timestamptz NOT NULL DEFAULT clock_timestamp(),updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),PRIMARY KEY(tenant_id,id),FOREIGN KEY(tenant_id,owner_id) REFERENCES archava_support.memberships(tenant_id,principal_id),FOREIGN KEY(tenant_id,assignee_id) REFERENCES archava_support.memberships(tenant_id,principal_id),CHECK(due_at IS NULL OR (isfinite(due_at) AND due_at>='2000-01-01T00:00:00Z'::timestamptz AND due_at<'2100-01-01T00:00:00Z'::timestamptz)),CHECK((state IN ('resolved','closed'))=(resolution IS NOT NULL)),CHECK((state IN ('resolved','closed'))=(resolved_at IS NOT NULL)),CHECK((state='closed')=(closed_at IS NOT NULL))
);
CREATE INDEX case_worklist ON archava_support.cases(tenant_id,owner_id,assignee_id,state,id);
CREATE INDEX case_due ON archava_support.cases(tenant_id,due_at,id) WHERE state IN ('open','in_progress','waiting');
CREATE TABLE archava_support.commands (
 tenant_id text NOT NULL,request_id uuid NOT NULL,actor_id text NOT NULL,fingerprint text NOT NULL,receipt jsonb,created_at timestamptz NOT NULL DEFAULT clock_timestamp(),PRIMARY KEY(tenant_id,request_id),FOREIGN KEY(tenant_id,actor_id) REFERENCES archava_support.memberships(tenant_id,principal_id)
);
CREATE TABLE archava_support.notes (
 tenant_id text NOT NULL,id uuid NOT NULL,case_id uuid NOT NULL,author_id text NOT NULL,body text NOT NULL CHECK(length(body) BETWEEN 1 AND 4000),request_id uuid NOT NULL,occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,request_id),FOREIGN KEY(tenant_id,case_id) REFERENCES archava_support.cases(tenant_id,id),FOREIGN KEY(tenant_id,author_id) REFERENCES archava_support.memberships(tenant_id,principal_id),FOREIGN KEY(tenant_id,request_id) REFERENCES archava_support.commands(tenant_id,request_id)
);
CREATE INDEX case_notes ON archava_support.notes(tenant_id,case_id,occurred_at DESC,id DESC);
CREATE TABLE archava_support.transitions (
 tenant_id text NOT NULL,request_id uuid NOT NULL,case_id uuid NOT NULL,actor_id text NOT NULL,operation text NOT NULL,previous_assignee_id text,assignee_id text,previous_priority text CHECK(previous_priority IN ('low','normal','high')),priority text NOT NULL CHECK(priority IN ('low','normal','high')),previous_due_at timestamptz,due_at timestamptz,previous_state text CHECK(previous_state IN ('open','in_progress','waiting','resolved','closed','cancelled')),next_state text NOT NULL CHECK(next_state IN ('open','in_progress','waiting','resolved','closed','cancelled')),cycle integer NOT NULL CHECK(cycle>0),case_revision integer NOT NULL CHECK(case_revision>0),previous_resolution text CHECK(length(previous_resolution) BETWEEN 1 AND 4000),resolution text CHECK(length(resolution) BETWEEN 1 AND 4000),reason text CHECK(length(reason) BETWEEN 1 AND 2000),occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),PRIMARY KEY(tenant_id,request_id),UNIQUE(tenant_id,case_id,case_revision),FOREIGN KEY(tenant_id,case_id) REFERENCES archava_support.cases(tenant_id,id),FOREIGN KEY(tenant_id,request_id) REFERENCES archava_support.commands(tenant_id,request_id)
);
CREATE TABLE archava_support.audit (
 tenant_id text NOT NULL,request_id uuid NOT NULL,actor_id text NOT NULL,operation text NOT NULL,record_id uuid NOT NULL,revision integer NOT NULL CHECK(revision>0),reason text CHECK(length(reason) BETWEEN 1 AND 2000),occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),PRIMARY KEY(tenant_id,request_id),UNIQUE(tenant_id,record_id,revision),FOREIGN KEY(tenant_id,record_id) REFERENCES archava_support.cases(tenant_id,id),FOREIGN KEY(tenant_id,request_id) REFERENCES archava_support.commands(tenant_id,request_id)
);
CREATE INDEX case_history ON archava_support.audit(tenant_id,record_id,revision DESC);
CREATE FUNCTION archava_support.immutable_history() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'Support history is immutable';END $$;
CREATE TRIGGER notes_immutable BEFORE UPDATE OR DELETE ON archava_support.notes FOR EACH ROW EXECUTE FUNCTION archava_support.immutable_history();
CREATE TRIGGER transitions_immutable BEFORE UPDATE OR DELETE ON archava_support.transitions FOR EACH ROW EXECUTE FUNCTION archava_support.immutable_history();
CREATE TRIGGER audit_immutable BEFORE UPDATE OR DELETE ON archava_support.audit FOR EACH ROW EXECUTE FUNCTION archava_support.immutable_history();
CREATE FUNCTION archava_support.guard_case() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF TG_OP='INSERT' THEN
  IF NEW.state<>'open' OR NEW.cycle<>1 OR NEW.revision<>1 OR NEW.assignee_id IS NOT NULL THEN RAISE EXCEPTION 'Case must start open and unassigned' USING ERRCODE='23514';END IF;
 ELSE
  IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.id IS DISTINCT FROM OLD.id OR NEW.owner_id IS DISTINCT FROM OLD.owner_id OR NEW.requester_name IS DISTINCT FROM OLD.requester_name OR NEW.subject IS DISTINCT FROM OLD.subject OR NEW.description IS DISTINCT FROM OLD.description OR NEW.created_at IS DISTINCT FROM OLD.created_at OR NEW.revision<>OLD.revision+1 THEN RAISE EXCEPTION 'Case identity/revision is immutable' USING ERRCODE='23514';END IF;
  IF NEW.state=OLD.state THEN
   IF NEW.cycle<>OLD.cycle OR NEW.resolution IS DISTINCT FROM OLD.resolution OR NEW.resolved_at IS DISTINCT FROM OLD.resolved_at OR NEW.closed_at IS DISTINCT FROM OLD.closed_at OR (OLD.state IN ('closed','cancelled') AND (NEW.assignee_id IS DISTINCT FROM OLD.assignee_id OR NEW.priority IS DISTINCT FROM OLD.priority OR NEW.due_at IS DISTINCT FROM OLD.due_at)) THEN RAISE EXCEPTION 'Case lifecycle evidence is immutable' USING ERRCODE='23514';END IF;
  ELSE
   IF NOT ((OLD.state='open' AND NEW.state IN ('in_progress','cancelled')) OR (OLD.state='in_progress' AND NEW.state IN ('waiting','resolved','cancelled')) OR (OLD.state='waiting' AND NEW.state IN ('in_progress','resolved','cancelled')) OR (OLD.state='resolved' AND NEW.state IN ('closed','open')) OR (OLD.state='closed' AND NEW.state='open')) OR NEW.assignee_id IS DISTINCT FROM OLD.assignee_id OR NEW.priority IS DISTINCT FROM OLD.priority THEN RAISE EXCEPTION 'Case transition refused' USING ERRCODE='23514';END IF;
   IF NEW.state='open' THEN
    IF NEW.cycle<>OLD.cycle+1 OR NEW.resolution IS NOT NULL OR NEW.resolved_at IS NOT NULL OR NEW.closed_at IS NOT NULL THEN RAISE EXCEPTION 'Reopen must create a clean resolution cycle' USING ERRCODE='23514';END IF;
   ELSE
    IF NEW.cycle<>OLD.cycle OR NEW.due_at IS DISTINCT FROM OLD.due_at OR (NEW.state='resolved' AND (NEW.resolution IS NULL OR NEW.resolved_at IS NULL OR NEW.closed_at IS NOT NULL)) OR (NEW.state='closed' AND (NEW.resolution IS DISTINCT FROM OLD.resolution OR NEW.resolved_at IS DISTINCT FROM OLD.resolved_at OR NEW.closed_at IS NULL)) OR (NEW.state NOT IN ('resolved','closed') AND (NEW.resolution IS DISTINCT FROM OLD.resolution OR NEW.resolved_at IS DISTINCT FROM OLD.resolved_at OR NEW.closed_at IS DISTINCT FROM OLD.closed_at)) THEN RAISE EXCEPTION 'Resolution evidence refused' USING ERRCODE='23514';END IF;
   END IF;
  END IF;
 END IF;
 IF NEW.assignee_id IS NOT NULL AND (TG_OP='INSERT' OR NEW.assignee_id IS DISTINCT FROM OLD.assignee_id) THEN
  PERFORM principal_id FROM archava_support.lock_membership(NEW.tenant_id,NEW.assignee_id) WHERE kind='human' AND active AND 'cases.read'=ANY(permissions);
  IF NOT FOUND THEN RAISE EXCEPTION 'Assignee must be an active human case reader' USING ERRCODE='23514';END IF;
 END IF;RETURN NEW;
END $$;
CREATE TRIGGER case_guard BEFORE INSERT OR UPDATE ON archava_support.cases FOR EACH ROW EXECUTE FUNCTION archava_support.guard_case();
CREATE FUNCTION archava_support.assert_case_integrity() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE c archava_support.cases;event archava_support.transitions;events integer;latest integer;
BEGIN
 SELECT * INTO c FROM archava_support.cases WHERE tenant_id=NEW.tenant_id AND id=NEW.id;
 SELECT count(*),max(revision) INTO events,latest FROM archava_support.audit WHERE tenant_id=c.tenant_id AND record_id=c.id;
 SELECT * INTO event FROM archava_support.transitions WHERE tenant_id=c.tenant_id AND case_id=c.id ORDER BY case_revision DESC LIMIT 1;
 IF events<>c.revision OR latest<>c.revision OR event.case_id IS NULL OR event.case_revision<>c.revision OR event.assignee_id IS DISTINCT FROM c.assignee_id OR event.priority IS DISTINCT FROM c.priority OR event.due_at IS DISTINCT FROM c.due_at OR event.next_state IS DISTINCT FROM c.state OR event.cycle IS DISTINCT FROM c.cycle OR event.resolution IS DISTINCT FROM c.resolution OR NOT EXISTS(SELECT 1 FROM archava_support.audit a JOIN archava_support.commands x ON x.tenant_id=a.tenant_id AND x.request_id=a.request_id WHERE a.tenant_id=c.tenant_id AND a.record_id=c.id AND a.revision=c.revision AND (x.receipt->>'id')=c.id::text AND (x.receipt->>'revision')::integer=c.revision)
 THEN RAISE EXCEPTION 'Case requires complete history and receipt' USING ERRCODE='23514';END IF;RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER case_complete AFTER INSERT OR UPDATE ON archava_support.cases DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_support.assert_case_integrity();
CREATE FUNCTION archava_support.guard_command() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF TG_OP='DELETE' OR OLD.receipt IS NOT NULL OR NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.request_id IS DISTINCT FROM OLD.request_id OR NEW.actor_id IS DISTINCT FROM OLD.actor_id OR NEW.fingerprint IS DISTINCT FROM OLD.fingerprint OR NEW.created_at IS DISTINCT FROM OLD.created_at THEN RAISE EXCEPTION 'Support command is immutable';END IF;RETURN NEW;
END $$;
CREATE TRIGGER command_guard BEFORE UPDATE OR DELETE ON archava_support.commands FOR EACH ROW EXECUTE FUNCTION archava_support.guard_command();
DO $$ DECLARE owned text;BEGIN
 FOREACH owned IN ARRAY ARRAY['memberships','cases','commands','notes','transitions','audit'] LOOP
  EXECUTE format('ALTER TABLE archava_support.%I ENABLE ROW LEVEL SECURITY',owned);
  EXECUTE format('ALTER TABLE archava_support.%I FORCE ROW LEVEL SECURITY',owned);
  EXECUTE format('CREATE POLICY support_tenant ON archava_support.%I USING(tenant_id=current_setting(''archava.tenant_id'',true)) WITH CHECK(tenant_id=current_setting(''archava.tenant_id'',true))',owned);
 END LOOP;
END $$;
CREATE FUNCTION archava_support.lock_membership(wanted_tenant text,wanted_principal text) RETURNS SETOF archava_support.memberships LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog AS $$
 SELECT * FROM archava_support.memberships WHERE tenant_id=wanted_tenant AND principal_id=wanted_principal AND wanted_tenant=current_setting('archava.tenant_id',true) FOR SHARE
$$;
REVOKE ALL ON FUNCTION archava_support.lock_membership(text,text) FROM PUBLIC;
