ALTER TABLE archava_scheduling.resources ADD COLUMN managed_by text CHECK(managed_by IS NULL OR managed_by='accommodation');
CREATE TABLE archava_scheduling.occupancy (
 tenant_id text NOT NULL, id uuid NOT NULL, resource_id uuid NOT NULL,
 owner_module text NOT NULL CHECK(owner_module IN ('timed','accommodation')),
 state text NOT NULL CHECK(state IN ('held','confirmed','cancelled')),
 starts_at timestamptz NOT NULL, ends_at timestamptz NOT NULL, expires_at timestamptz,
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(), updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,id), UNIQUE(tenant_id,resource_id,id),
 FOREIGN KEY(tenant_id,resource_id) REFERENCES archava_scheduling.resources(tenant_id,id),
 CHECK(isfinite(starts_at) AND isfinite(ends_at) AND ends_at>starts_at AND ends_at-starts_at<=interval '32 days'),
 CHECK(state<>'held' OR (expires_at IS NOT NULL AND isfinite(expires_at)))
);
-- Retain all prior timed rows, including historical cancelled/expired holds. No new business command is created.
INSERT INTO archava_scheduling.occupancy(tenant_id,id,resource_id,owner_module,state,starts_at,ends_at,expires_at,created_at,updated_at)
 SELECT tenant_id,id,resource_id,'timed',state,starts_at,ends_at,expires_at,created_at,updated_at FROM archava_scheduling.reservations;
CREATE INDEX occupancy_slots ON archava_scheduling.occupancy(tenant_id,resource_id,starts_at,ends_at) WHERE state IN ('held','confirmed');
ALTER TABLE archava_scheduling.occupancy ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_scheduling.occupancy FORCE ROW LEVEL SECURITY;
CREATE POLICY occupancy_tenant ON archava_scheduling.occupancy USING(tenant_id=current_setting('archava.tenant_id',true)) WITH CHECK(tenant_id=current_setting('archava.tenant_id',true));
CREATE FUNCTION archava_scheduling.guard_occupancy() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE enabled boolean; managed text; observed timestamptz;
BEGIN
 SELECT active,managed_by INTO enabled,managed FROM archava_scheduling.resources WHERE tenant_id=NEW.tenant_id AND id=NEW.resource_id FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Unknown occupancy resource' USING ERRCODE='23503'; END IF;
 observed:=clock_timestamp();
 IF (NEW.owner_module='timed' AND managed IS NOT NULL) OR (NEW.owner_module='accommodation' AND managed IS DISTINCT FROM 'accommodation')
 THEN RAISE EXCEPTION 'Wrong occupancy owner' USING ERRCODE='23514'; END IF;
 IF TG_OP='UPDATE' THEN
  IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.id IS DISTINCT FROM OLD.id OR NEW.resource_id IS DISTINCT FROM OLD.resource_id
   OR NEW.owner_module IS DISTINCT FROM OLD.owner_module OR NEW.created_at IS DISTINCT FROM OLD.created_at OR NEW.expires_at IS DISTINCT FROM OLD.expires_at
  THEN RAISE EXCEPTION 'Occupancy identity is immutable' USING ERRCODE='23514'; END IF;
  IF OLD.state='cancelled' OR (OLD.state='held' AND OLD.expires_at<=observed) OR (OLD.state='confirmed' AND NEW.state='held')
  THEN RAISE EXCEPTION 'Occupancy transition refused' USING ERRCODE='23514'; END IF;
 END IF;
 IF NEW.state IN ('held','confirmed') THEN
  IF NOT enabled OR (NEW.state='held' AND NEW.expires_at<=observed) OR (TG_OP='INSERT' AND NEW.ends_at<=observed)
  THEN RAISE EXCEPTION 'Inactive resource or invalid occupancy time' USING ERRCODE='23514'; END IF;
  IF EXISTS(SELECT 1 FROM archava_scheduling.occupancy o WHERE o.tenant_id=NEW.tenant_id AND o.resource_id=NEW.resource_id AND o.id<>NEW.id
   AND (o.state='confirmed' OR (o.state='held' AND o.expires_at>observed)) AND o.starts_at<NEW.ends_at AND o.ends_at>NEW.starts_at)
  THEN RAISE EXCEPTION 'Shared occupancy overlaps' USING ERRCODE='23P01'; END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER occupancy_guard BEFORE INSERT OR UPDATE ON archava_scheduling.occupancy FOR EACH ROW EXECUTE FUNCTION archava_scheduling.guard_occupancy();
CREATE FUNCTION archava_scheduling.sync_timed_occupancy() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 INSERT INTO archava_scheduling.occupancy(tenant_id,id,resource_id,owner_module,state,starts_at,ends_at,expires_at,created_at,updated_at)
  VALUES(NEW.tenant_id,NEW.id,NEW.resource_id,'timed',NEW.state,NEW.starts_at,NEW.ends_at,NEW.expires_at,NEW.created_at,NEW.updated_at)
 ON CONFLICT(tenant_id,id) DO UPDATE SET state=excluded.state,starts_at=excluded.starts_at,ends_at=excluded.ends_at,updated_at=excluded.updated_at;
 RETURN NEW;
END $$;
CREATE TRIGGER timed_occupancy_sync AFTER INSERT OR UPDATE ON archava_scheduling.reservations FOR EACH ROW EXECUTE FUNCTION archava_scheduling.sync_timed_occupancy();
CREATE OR REPLACE FUNCTION archava_scheduling.guard_resource() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE observed timestamptz:=clock_timestamp();
BEGIN
 IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.id IS DISTINCT FROM OLD.id OR NEW.owner_id IS DISTINCT FROM OLD.owner_id
 OR NEW.kind IS DISTINCT FROM OLD.kind OR NEW.timezone IS DISTINCT FROM OLD.timezone OR NEW.created_at IS DISTINCT FROM OLD.created_at OR NEW.managed_by IS DISTINCT FROM OLD.managed_by
 THEN RAISE EXCEPTION 'Resource identity is immutable' USING ERRCODE='23514'; END IF;
 IF NOT NEW.active AND EXISTS(SELECT 1 FROM archava_scheduling.occupancy o WHERE o.tenant_id=OLD.tenant_id AND o.resource_id=OLD.id
  AND o.ends_at>observed AND (o.state='confirmed' OR (o.state='held' AND o.expires_at>observed)))
 THEN RAISE EXCEPTION 'Resource has live occupancy' USING ERRCODE='23514'; END IF;
 RETURN NEW;
END $$;
-- This bounded factory owns a fixed internal service identity with no domain grants.
CREATE FUNCTION archava_scheduling.create_accommodation_resource(wanted_tenant text,wanted_id uuid,wanted_title text,wanted_timezone text)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog AS $$
BEGIN
 IF wanted_tenant IS DISTINCT FROM current_setting('archava.tenant_id',true) THEN RAISE EXCEPTION 'Wrong tenant' USING ERRCODE='42501'; END IF;
 INSERT INTO archava_scheduling.memberships(tenant_id,principal_id,kind,permissions,scope)
 VALUES(wanted_tenant,'accommodation-module','service',ARRAY[]::text[],'all') ON CONFLICT DO NOTHING;
 IF NOT EXISTS(SELECT 1 FROM archava_scheduling.memberships WHERE tenant_id=wanted_tenant AND principal_id='accommodation-module' AND kind='service' AND permissions=ARRAY[]::text[] AND active)
 THEN RAISE EXCEPTION 'Invalid internal occupancy identity' USING ERRCODE='23514'; END IF;
 INSERT INTO archava_scheduling.resources(tenant_id,id,owner_id,title,kind,timezone,managed_by)
 VALUES(wanted_tenant,wanted_id,'accommodation-module',wanted_title,'room',wanted_timezone,'accommodation');
END $$;
REVOKE ALL ON FUNCTION archava_scheduling.create_accommodation_resource(text,uuid,text,text) FROM PUBLIC;
