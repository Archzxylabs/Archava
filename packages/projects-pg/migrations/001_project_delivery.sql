CREATE TABLE archava_projects.memberships (
 tenant_id text NOT NULL, principal_id text NOT NULL,
 kind text NOT NULL CHECK(kind IN ('human','service')),
 permissions text[] NOT NULL CHECK(permissions <@ ARRAY['projects.read','projects.write','tasks.read','tasks.write','milestones.read','milestones.write','audit.read']::text[]),
 scope text NOT NULL CHECK(scope IN ('all','assigned')), active boolean NOT NULL DEFAULT true,
 PRIMARY KEY(tenant_id,principal_id)
);
CREATE TABLE archava_projects.projects (
 tenant_id text NOT NULL, id uuid NOT NULL, owner_id text NOT NULL,
 title text NOT NULL CHECK(length(title) BETWEEN 1 AND 160),
 scope_summary text NOT NULL CHECK(length(scope_summary) BETWEEN 1 AND 2000),
 scope_version integer NOT NULL DEFAULT 1 CHECK(scope_version=1),
 scope_confirmed_at timestamptz, scope_confirmed_by text,
 state text NOT NULL DEFAULT 'draft' CHECK(state IN ('draft','active','review','delivered','closed','cancelled')),
 revision integer NOT NULL DEFAULT 1 CHECK(revision>0), due_at timestamptz,
 created_at timestamptz NOT NULL DEFAULT now(), updated_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY(tenant_id,id),
 FOREIGN KEY(tenant_id,owner_id) REFERENCES archava_projects.memberships(tenant_id,principal_id),
 FOREIGN KEY(tenant_id,scope_confirmed_by) REFERENCES archava_projects.memberships(tenant_id,principal_id),
 CHECK((scope_confirmed_at IS NULL) = (scope_confirmed_by IS NULL)),
 CHECK(state IN ('draft','cancelled') OR scope_confirmed_at IS NOT NULL)
);
CREATE TABLE archava_projects.milestones (
 tenant_id text NOT NULL, id uuid NOT NULL, project_id uuid NOT NULL,
 title text NOT NULL CHECK(length(title) BETWEEN 1 AND 160), required boolean NOT NULL,
 state text NOT NULL DEFAULT 'open' CHECK(state IN ('open','reached')),
 revision integer NOT NULL DEFAULT 1 CHECK(revision>0), due_at timestamptz,
 reached_at timestamptz, evidence text CHECK(length(evidence) BETWEEN 1 AND 2000),
 created_at timestamptz NOT NULL DEFAULT now(), updated_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY(tenant_id,id), UNIQUE(tenant_id,project_id,id),
 FOREIGN KEY(tenant_id,project_id) REFERENCES archava_projects.projects(tenant_id,id),
 CHECK((state='reached') = (reached_at IS NOT NULL AND evidence IS NOT NULL))
);
CREATE TABLE archava_projects.tasks (
 tenant_id text NOT NULL, id uuid NOT NULL, project_id uuid NOT NULL, milestone_id uuid,
 title text NOT NULL CHECK(length(title) BETWEEN 1 AND 160), required boolean NOT NULL,
 state text NOT NULL DEFAULT 'todo' CHECK(state IN ('todo','doing','done','cancelled')),
 revision integer NOT NULL DEFAULT 1 CHECK(revision>0), due_at timestamptz,
 created_at timestamptz NOT NULL DEFAULT now(), updated_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY(tenant_id,id),
 FOREIGN KEY(tenant_id,project_id) REFERENCES archava_projects.projects(tenant_id,id),
 FOREIGN KEY(tenant_id,project_id,milestone_id) REFERENCES archava_projects.milestones(tenant_id,project_id,id)
);
CREATE TABLE archava_projects.commands (
 tenant_id text NOT NULL, request_id uuid NOT NULL, actor_id text NOT NULL,
 fingerprint text NOT NULL, receipt jsonb, created_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY(tenant_id,request_id),
 FOREIGN KEY(tenant_id,actor_id) REFERENCES archava_projects.memberships(tenant_id,principal_id)
);
CREATE TABLE archava_projects.audit (
 tenant_id text NOT NULL, request_id uuid NOT NULL, actor_id text NOT NULL,
 operation text NOT NULL, project_id uuid NOT NULL, resource_id uuid NOT NULL,
 revision integer NOT NULL CHECK(revision>0), project_revision integer NOT NULL CHECK(project_revision>0),
 reason text CHECK(length(reason) BETWEEN 1 AND 2000), occurred_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY(tenant_id,request_id),
 FOREIGN KEY(tenant_id,request_id) REFERENCES archava_projects.commands(tenant_id,request_id),
 FOREIGN KEY(tenant_id,project_id) REFERENCES archava_projects.projects(tenant_id,id)
);
CREATE INDEX projects_owner ON archava_projects.projects(tenant_id,owner_id,id);
CREATE INDEX project_tasks ON archava_projects.tasks(tenant_id,project_id,id);
CREATE INDEX project_milestones ON archava_projects.milestones(tenant_id,project_id,id);
CREATE INDEX project_history ON archava_projects.audit(tenant_id,project_id,project_revision DESC);
CREATE FUNCTION archava_projects.prevent_scope_change() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.id IS DISTINCT FROM OLD.id
 OR NEW.owner_id IS DISTINCT FROM OLD.owner_id OR NEW.scope_summary IS DISTINCT FROM OLD.scope_summary
 OR NEW.scope_version IS DISTINCT FROM OLD.scope_version THEN RAISE EXCEPTION 'Project scope and ownership are immutable'; END IF;
 IF OLD.scope_confirmed_at IS NOT NULL AND (NEW.scope_confirmed_at IS DISTINCT FROM OLD.scope_confirmed_at
 OR NEW.scope_confirmed_by IS DISTINCT FROM OLD.scope_confirmed_by) THEN RAISE EXCEPTION 'Project scope confirmation is immutable'; END IF;
 IF OLD.scope_confirmed_at IS NULL AND NEW.scope_confirmed_at IS NOT NULL AND NEW.state<>'active'
 THEN RAISE EXCEPTION 'Project activation must confirm scope'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER project_scope_immutable BEFORE UPDATE ON archava_projects.projects
 FOR EACH ROW EXECUTE FUNCTION archava_projects.prevent_scope_change();
CREATE FUNCTION archava_projects.prevent_audit_change() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN RAISE EXCEPTION 'Project audit is append-only'; END $$;
CREATE TRIGGER project_audit_immutable BEFORE UPDATE OR DELETE ON archava_projects.audit
 FOR EACH ROW EXECUTE FUNCTION archava_projects.prevent_audit_change();
CREATE FUNCTION archava_projects.prevent_command_change() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF OLD.receipt IS NOT NULL OR TG_OP='DELETE' THEN RAISE EXCEPTION 'Committed project command is immutable'; END IF;
 IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.request_id IS DISTINCT FROM OLD.request_id
 OR NEW.actor_id IS DISTINCT FROM OLD.actor_id OR NEW.fingerprint IS DISTINCT FROM OLD.fingerprint
 OR NEW.created_at IS DISTINCT FROM OLD.created_at THEN RAISE EXCEPTION 'Project command identity is immutable'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER project_command_immutable BEFORE UPDATE OR DELETE ON archava_projects.commands
 FOR EACH ROW EXECUTE FUNCTION archava_projects.prevent_command_change();
ALTER TABLE archava_projects.memberships ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_projects.memberships FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_projects.projects ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_projects.projects FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_projects.tasks ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_projects.tasks FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_projects.milestones ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_projects.milestones FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_projects.commands ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_projects.commands FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_projects.audit ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_projects.audit FORCE ROW LEVEL SECURITY;
CREATE POLICY project_membership_tenant ON archava_projects.memberships USING(tenant_id=current_setting('archava.tenant_id',true)) WITH CHECK(tenant_id=current_setting('archava.tenant_id',true));
CREATE POLICY project_record_tenant ON archava_projects.projects USING(tenant_id=current_setting('archava.tenant_id',true)) WITH CHECK(tenant_id=current_setting('archava.tenant_id',true));
CREATE POLICY project_task_tenant ON archava_projects.tasks USING(tenant_id=current_setting('archava.tenant_id',true)) WITH CHECK(tenant_id=current_setting('archava.tenant_id',true));
CREATE POLICY project_milestone_tenant ON archava_projects.milestones USING(tenant_id=current_setting('archava.tenant_id',true)) WITH CHECK(tenant_id=current_setting('archava.tenant_id',true));
CREATE POLICY project_command_tenant ON archava_projects.commands USING(tenant_id=current_setting('archava.tenant_id',true)) WITH CHECK(tenant_id=current_setting('archava.tenant_id',true));
CREATE POLICY project_audit_tenant ON archava_projects.audit USING(tenant_id=current_setting('archava.tenant_id',true)) WITH CHECK(tenant_id=current_setting('archava.tenant_id',true));
CREATE FUNCTION archava_projects.lock_membership(wanted_tenant text,wanted_principal text)
RETURNS SETOF archava_projects.memberships LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog AS $$
 SELECT * FROM archava_projects.memberships WHERE tenant_id=wanted_tenant AND principal_id=wanted_principal
 AND wanted_tenant=current_setting('archava.tenant_id',true) FOR SHARE
$$;
REVOKE ALL ON FUNCTION archava_projects.lock_membership(text,text) FROM PUBLIC;
