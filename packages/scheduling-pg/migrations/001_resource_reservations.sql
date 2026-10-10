CREATE TABLE archava_scheduling.memberships (
 tenant_id text NOT NULL, principal_id text NOT NULL, kind text NOT NULL CHECK(kind IN ('human','service')),
 permissions text[] NOT NULL CHECK(permissions <@ ARRAY['resources.read','resources.write','reservations.read','reservations.write','audit.read']::text[]),
 scope text NOT NULL CHECK(scope IN ('all','assigned')), active boolean NOT NULL DEFAULT true,
 PRIMARY KEY(tenant_id,principal_id)
);
CREATE TABLE archava_scheduling.resources (
 tenant_id text NOT NULL, id uuid NOT NULL, owner_id text NOT NULL,
 title text NOT NULL CHECK(length(title) BETWEEN 1 AND 160),
 kind text NOT NULL CHECK(kind IN ('staff','asset','room','space')),
 timezone text NOT NULL CHECK(length(timezone) BETWEEN 1 AND 100), capacity integer NOT NULL DEFAULT 1 CHECK(capacity=1),
 active boolean NOT NULL DEFAULT true, revision integer NOT NULL DEFAULT 1 CHECK(revision>0),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(), updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,id), FOREIGN KEY(tenant_id,owner_id) REFERENCES archava_scheduling.memberships(tenant_id,principal_id)
);
CREATE TABLE archava_scheduling.reservations (
 tenant_id text NOT NULL, id uuid NOT NULL, resource_id uuid NOT NULL, owner_id text NOT NULL,
 title text NOT NULL CHECK(length(title) BETWEEN 1 AND 160), kind text NOT NULL CHECK(kind IN ('booking','block')),
 state text NOT NULL CHECK(state IN ('held','confirmed','cancelled')),
 starts_at timestamptz NOT NULL, ends_at timestamptz NOT NULL, expires_at timestamptz,
 revision integer NOT NULL DEFAULT 1 CHECK(revision>0),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(), updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,id), FOREIGN KEY(tenant_id,resource_id) REFERENCES archava_scheduling.resources(tenant_id,id),
 FOREIGN KEY(tenant_id,owner_id) REFERENCES archava_scheduling.memberships(tenant_id,principal_id),
 CHECK(isfinite(starts_at) AND isfinite(ends_at) AND ends_at>starts_at AND ends_at-starts_at<=interval '31 days'),
 CHECK(state<>'held' OR (expires_at IS NOT NULL AND kind='booking')),
 CHECK(kind<>'block' OR (state<>'held' AND expires_at IS NULL))
);
CREATE TABLE archava_scheduling.commands (
 tenant_id text NOT NULL, request_id uuid NOT NULL, actor_id text NOT NULL, fingerprint text NOT NULL,
 receipt jsonb, created_at timestamptz NOT NULL DEFAULT clock_timestamp(), PRIMARY KEY(tenant_id,request_id),
 FOREIGN KEY(tenant_id,actor_id) REFERENCES archava_scheduling.memberships(tenant_id,principal_id)
);
CREATE TABLE archava_scheduling.audit (
 tenant_id text NOT NULL, request_id uuid NOT NULL, actor_id text NOT NULL, operation text NOT NULL,
 resource_id uuid NOT NULL, record_id uuid NOT NULL, revision integer NOT NULL CHECK(revision>0),
 reason text CHECK(length(reason) BETWEEN 1 AND 2000), occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,request_id), FOREIGN KEY(tenant_id,request_id) REFERENCES archava_scheduling.commands(tenant_id,request_id),
 FOREIGN KEY(tenant_id,resource_id) REFERENCES archava_scheduling.resources(tenant_id,id)
);
CREATE INDEX resource_owner ON archava_scheduling.resources(tenant_id,owner_id,id);
CREATE INDEX reservation_slots ON archava_scheduling.reservations(tenant_id,resource_id,starts_at,ends_at) WHERE state IN ('held','confirmed');
CREATE INDEX reservation_history ON archava_scheduling.audit(tenant_id,record_id,occurred_at DESC,request_id);

-- Serialize every write for a resource, including raw SQL writes under the runtime role.
-- clock_timestamp() is sampled AFTER the resource lock: transaction start time is unsafe for expiring holds.
CREATE FUNCTION archava_scheduling.guard_reservation() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE enabled boolean; observed timestamptz;
BEGIN
 SELECT active INTO enabled FROM archava_scheduling.resources WHERE tenant_id=NEW.tenant_id AND id=NEW.resource_id FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Unknown resource' USING ERRCODE='23503'; END IF;
 observed:=clock_timestamp();
 IF TG_OP='UPDATE' THEN
  IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.id IS DISTINCT FROM OLD.id
  OR NEW.resource_id IS DISTINCT FROM OLD.resource_id OR NEW.owner_id IS DISTINCT FROM OLD.owner_id
  OR NEW.kind IS DISTINCT FROM OLD.kind OR NEW.created_at IS DISTINCT FROM OLD.created_at
  OR NEW.expires_at IS DISTINCT FROM OLD.expires_at THEN RAISE EXCEPTION 'Reservation identity is immutable' USING ERRCODE='23514'; END IF;
  IF OLD.state='cancelled' OR (OLD.state='held' AND OLD.expires_at<=observed)
  THEN RAISE EXCEPTION 'Terminal reservation' USING ERRCODE='23514'; END IF;
  IF OLD.state='confirmed' AND NEW.state='held' THEN RAISE EXCEPTION 'Invalid reservation transition' USING ERRCODE='23514'; END IF;
  IF OLD.starts_at<=observed AND (NEW.starts_at IS DISTINCT FROM OLD.starts_at OR NEW.ends_at IS DISTINCT FROM OLD.ends_at)
  THEN RAISE EXCEPTION 'Started reservation cannot move' USING ERRCODE='23514'; END IF;
  IF NEW.starts_at IS DISTINCT FROM OLD.starts_at AND NEW.starts_at<=observed
  THEN RAISE EXCEPTION 'Cannot move into the past' USING ERRCODE='23514'; END IF;
 END IF;
 IF NEW.state IN ('held','confirmed') THEN
  IF NOT enabled OR (TG_OP='INSERT' AND NEW.starts_at<=observed)
  OR (NEW.state='held' AND NEW.expires_at<=observed)
  THEN RAISE EXCEPTION 'Unavailable resource or expired hold' USING ERRCODE='23514'; END IF;
  IF EXISTS(SELECT 1 FROM archava_scheduling.reservations r WHERE r.tenant_id=NEW.tenant_id AND r.resource_id=NEW.resource_id
   AND r.id<>NEW.id AND (r.state='confirmed' OR (r.state='held' AND r.expires_at>observed))
   AND r.starts_at<NEW.ends_at AND r.ends_at>NEW.starts_at)
  THEN RAISE EXCEPTION 'Resource interval overlaps' USING ERRCODE='23P01'; END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER reservation_guard BEFORE INSERT OR UPDATE ON archava_scheduling.reservations FOR EACH ROW EXECUTE FUNCTION archava_scheduling.guard_reservation();
CREATE FUNCTION archava_scheduling.guard_resource() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE observed timestamptz:=clock_timestamp();
BEGIN
 IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.id IS DISTINCT FROM OLD.id OR NEW.owner_id IS DISTINCT FROM OLD.owner_id
 OR NEW.kind IS DISTINCT FROM OLD.kind OR NEW.timezone IS DISTINCT FROM OLD.timezone OR NEW.created_at IS DISTINCT FROM OLD.created_at
 THEN RAISE EXCEPTION 'Resource identity is immutable' USING ERRCODE='23514'; END IF;
 IF NOT NEW.active AND EXISTS(SELECT 1 FROM archava_scheduling.reservations r WHERE r.tenant_id=OLD.tenant_id AND r.resource_id=OLD.id
  AND r.ends_at>observed AND (r.state='confirmed' OR (r.state='held' AND r.expires_at>observed)))
 THEN RAISE EXCEPTION 'Resource has live reservations' USING ERRCODE='23514'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER resource_guard BEFORE UPDATE ON archava_scheduling.resources FOR EACH ROW EXECUTE FUNCTION archava_scheduling.guard_resource();
CREATE FUNCTION archava_scheduling.prevent_audit_change() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN RAISE EXCEPTION 'Scheduling audit is append-only'; END $$;
CREATE TRIGGER scheduling_audit_immutable BEFORE UPDATE OR DELETE ON archava_scheduling.audit FOR EACH ROW EXECUTE FUNCTION archava_scheduling.prevent_audit_change();
CREATE FUNCTION archava_scheduling.prevent_command_change() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF OLD.receipt IS NOT NULL OR TG_OP='DELETE' OR NEW.tenant_id IS DISTINCT FROM OLD.tenant_id
 OR NEW.request_id IS DISTINCT FROM OLD.request_id OR NEW.actor_id IS DISTINCT FROM OLD.actor_id
 OR NEW.fingerprint IS DISTINCT FROM OLD.fingerprint OR NEW.created_at IS DISTINCT FROM OLD.created_at
 THEN RAISE EXCEPTION 'Scheduling command is immutable'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER scheduling_command_immutable BEFORE UPDATE OR DELETE ON archava_scheduling.commands FOR EACH ROW EXECUTE FUNCTION archava_scheduling.prevent_command_change();
ALTER TABLE archava_scheduling.memberships ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_scheduling.memberships FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_scheduling.resources ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_scheduling.resources FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_scheduling.reservations ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_scheduling.reservations FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_scheduling.commands ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_scheduling.commands FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_scheduling.audit ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_scheduling.audit FORCE ROW LEVEL SECURITY;
CREATE POLICY scheduling_member_tenant ON archava_scheduling.memberships USING(tenant_id=current_setting('archava.tenant_id',true)) WITH CHECK(tenant_id=current_setting('archava.tenant_id',true));
CREATE POLICY scheduling_resource_tenant ON archava_scheduling.resources USING(tenant_id=current_setting('archava.tenant_id',true)) WITH CHECK(tenant_id=current_setting('archava.tenant_id',true));
CREATE POLICY scheduling_reservation_tenant ON archava_scheduling.reservations USING(tenant_id=current_setting('archava.tenant_id',true)) WITH CHECK(tenant_id=current_setting('archava.tenant_id',true));
CREATE POLICY scheduling_command_tenant ON archava_scheduling.commands USING(tenant_id=current_setting('archava.tenant_id',true)) WITH CHECK(tenant_id=current_setting('archava.tenant_id',true));
CREATE POLICY scheduling_audit_tenant ON archava_scheduling.audit USING(tenant_id=current_setting('archava.tenant_id',true)) WITH CHECK(tenant_id=current_setting('archava.tenant_id',true));
CREATE FUNCTION archava_scheduling.lock_membership(wanted_tenant text,wanted_principal text)
RETURNS SETOF archava_scheduling.memberships LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog AS $$
 SELECT * FROM archava_scheduling.memberships WHERE tenant_id=wanted_tenant AND principal_id=wanted_principal
 AND wanted_tenant=current_setting('archava.tenant_id',true) FOR SHARE
$$;
REVOKE ALL ON FUNCTION archava_scheduling.lock_membership(text,text) FROM PUBLIC;
