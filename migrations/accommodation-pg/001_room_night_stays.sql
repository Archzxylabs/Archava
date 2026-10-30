CREATE TABLE archava_accommodation.memberships (
 tenant_id text NOT NULL, principal_id text NOT NULL, kind text NOT NULL CHECK(kind IN ('human','service')),
 permissions text[] NOT NULL CHECK(permissions <@ ARRAY['properties.read','properties.write','rooms.read','rooms.write','housekeeping.write','rooms.inspect','stays.read','stays.write','audit.read']::text[]),
 scope text NOT NULL CHECK(scope IN ('all','assigned')), active boolean NOT NULL DEFAULT true, PRIMARY KEY(tenant_id,principal_id)
);
CREATE TABLE archava_accommodation.properties (
 tenant_id text NOT NULL, id uuid NOT NULL, owner_id text NOT NULL,
 title text NOT NULL CHECK(length(title) BETWEEN 1 AND 160), timezone text NOT NULL,
 check_in_time text NOT NULL CHECK(check_in_time ~ '^([01][0-9]|2[0-3]):[0-5][0-9]$'),
 check_out_time text NOT NULL CHECK(check_out_time ~ '^([01][0-9]|2[0-3]):[0-5][0-9]$'),
 currency text NOT NULL DEFAULT 'IDR' CHECK(currency='IDR'), revision integer NOT NULL DEFAULT 1 CHECK(revision>0),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(), updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,id), FOREIGN KEY(tenant_id,owner_id) REFERENCES archava_accommodation.memberships(tenant_id,principal_id),
 CHECK(check_out_time<=check_in_time)
);
CREATE TABLE archava_accommodation.room_types (
 tenant_id text NOT NULL, id uuid NOT NULL, property_id uuid NOT NULL, title text NOT NULL CHECK(length(title) BETWEEN 1 AND 160),
 max_guests integer NOT NULL CHECK(max_guests BETWEEN 1 AND 16), nightly_rate integer NOT NULL CHECK(nightly_rate BETWEEN 0 AND 1000000000),
 revision integer NOT NULL DEFAULT 1 CHECK(revision>0), created_at timestamptz NOT NULL DEFAULT clock_timestamp(), updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,id), UNIQUE(tenant_id,property_id,id), FOREIGN KEY(tenant_id,property_id) REFERENCES archava_accommodation.properties(tenant_id,id)
);
CREATE TABLE archava_accommodation.rooms (
 tenant_id text NOT NULL, id uuid NOT NULL, property_id uuid NOT NULL, room_type_id uuid NOT NULL,
 code text NOT NULL CHECK(length(code) BETWEEN 1 AND 40), active boolean NOT NULL DEFAULT true,
 housekeeping text NOT NULL DEFAULT 'dirty' CHECK(housekeeping IN ('dirty','clean','inspected')), revision integer NOT NULL DEFAULT 1 CHECK(revision>0),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(), updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,id), UNIQUE(tenant_id,property_id,id), UNIQUE(tenant_id,property_id,code),
 FOREIGN KEY(tenant_id,property_id) REFERENCES archava_accommodation.properties(tenant_id,id),
 FOREIGN KEY(tenant_id,property_id,room_type_id) REFERENCES archava_accommodation.room_types(tenant_id,property_id,id),
 FOREIGN KEY(tenant_id,id) REFERENCES archava_scheduling.resources(tenant_id,id)
);
CREATE TABLE archava_accommodation.stays (
 tenant_id text NOT NULL, id uuid NOT NULL, property_id uuid NOT NULL, room_id uuid NOT NULL, owner_id text NOT NULL,
 guest_name text NOT NULL CHECK(length(guest_name) BETWEEN 1 AND 160), guests integer NOT NULL CHECK(guests BETWEEN 1 AND 16),
 arrival date NOT NULL, departure date NOT NULL, nights integer NOT NULL CHECK(nights BETWEEN 1 AND 31),
 nightly_rate integer NOT NULL CHECK(nightly_rate BETWEEN 0 AND 1000000000), rate_revision integer NOT NULL CHECK(rate_revision>0),
 total_estimate bigint NOT NULL, currency text NOT NULL DEFAULT 'IDR' CHECK(currency='IDR'),
 state text NOT NULL DEFAULT 'held' CHECK(state IN ('held','confirmed','in_house','checked_out','cancelled')),
 starts_at timestamptz NOT NULL, ends_at timestamptz NOT NULL, expires_at timestamptz NOT NULL,
 checked_in_at timestamptz, checked_out_at timestamptz, revision integer NOT NULL DEFAULT 1 CHECK(revision>0),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(), updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,id), FOREIGN KEY(tenant_id,property_id,room_id) REFERENCES archava_accommodation.rooms(tenant_id,property_id,id),
 FOREIGN KEY(tenant_id,owner_id) REFERENCES archava_accommodation.memberships(tenant_id,principal_id),
 FOREIGN KEY(tenant_id,room_id,id) REFERENCES archava_scheduling.occupancy(tenant_id,resource_id,id),
 CHECK(isfinite(arrival) AND isfinite(departure) AND nights=departure-arrival AND total_estimate=nightly_rate::bigint*nights),
 CHECK(isfinite(starts_at) AND isfinite(ends_at) AND ends_at>starts_at AND isfinite(expires_at)),
 CHECK((state IN ('in_house','checked_out'))=(checked_in_at IS NOT NULL)),
 CHECK((state='checked_out')=(checked_out_at IS NOT NULL)), CHECK(checked_out_at IS NULL OR checked_out_at>=checked_in_at)
);
CREATE UNIQUE INDEX one_physical_occupant ON archava_accommodation.stays(tenant_id,room_id) WHERE state='in_house';
CREATE TABLE archava_accommodation.stay_nights (
 tenant_id text NOT NULL, stay_id uuid NOT NULL, night_date date NOT NULL, rate integer NOT NULL CHECK(rate BETWEEN 0 AND 1000000000), rate_revision integer NOT NULL CHECK(rate_revision>0),
 PRIMARY KEY(tenant_id,stay_id,night_date), FOREIGN KEY(tenant_id,stay_id) REFERENCES archava_accommodation.stays(tenant_id,id)
);
CREATE TABLE archava_accommodation.commands (
 tenant_id text NOT NULL, request_id uuid NOT NULL, actor_id text NOT NULL, fingerprint text NOT NULL, receipt jsonb,
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(), PRIMARY KEY(tenant_id,request_id),
 FOREIGN KEY(tenant_id,actor_id) REFERENCES archava_accommodation.memberships(tenant_id,principal_id)
);
CREATE TABLE archava_accommodation.audit (
 tenant_id text NOT NULL, request_id uuid NOT NULL, actor_id text NOT NULL, operation text NOT NULL,
 property_id uuid NOT NULL, record_id uuid NOT NULL, revision integer NOT NULL CHECK(revision>0), reason text CHECK(length(reason) BETWEEN 1 AND 2000),
 occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(), PRIMARY KEY(tenant_id,request_id),
 FOREIGN KEY(tenant_id,request_id) REFERENCES archava_accommodation.commands(tenant_id,request_id),
 FOREIGN KEY(tenant_id,property_id) REFERENCES archava_accommodation.properties(tenant_id,id)
);
CREATE INDEX property_owner ON archava_accommodation.properties(tenant_id,owner_id,id);
CREATE INDEX room_types_property ON archava_accommodation.room_types(tenant_id,property_id,id);
CREATE INDEX rooms_property ON archava_accommodation.rooms(tenant_id,property_id,id);
CREATE INDEX stays_property ON archava_accommodation.stays(tenant_id,property_id,id);
CREATE INDEX stays_dates ON archava_accommodation.stays(tenant_id,room_id,arrival,departure);
CREATE INDEX accommodation_history ON archava_accommodation.audit(tenant_id,record_id,occurred_at DESC,request_id);
CREATE FUNCTION archava_accommodation.guard_room_type() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.id IS DISTINCT FROM OLD.id OR NEW.property_id IS DISTINCT FROM OLD.property_id
 OR NEW.title IS DISTINCT FROM OLD.title OR NEW.max_guests IS DISTINCT FROM OLD.max_guests OR NEW.created_at IS DISTINCT FROM OLD.created_at OR NEW.revision<>OLD.revision+1
 THEN RAISE EXCEPTION 'Room type identity/revision is immutable' USING ERRCODE='23514'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER room_type_guard BEFORE UPDATE ON archava_accommodation.room_types FOR EACH ROW EXECUTE FUNCTION archava_accommodation.guard_room_type();
CREATE FUNCTION archava_accommodation.guard_room() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 PERFORM id FROM archava_scheduling.resources WHERE tenant_id=NEW.tenant_id AND id=NEW.id AND managed_by='accommodation' FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Wrong room resource' USING ERRCODE='23514'; END IF;
 IF TG_OP='UPDATE' THEN
  IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.id IS DISTINCT FROM OLD.id OR NEW.property_id IS DISTINCT FROM OLD.property_id OR NEW.room_type_id IS DISTINCT FROM OLD.room_type_id
  OR NEW.code IS DISTINCT FROM OLD.code OR NEW.created_at IS DISTINCT FROM OLD.created_at OR NEW.revision<>OLD.revision+1
  THEN RAISE EXCEPTION 'Room identity/revision is immutable' USING ERRCODE='23514'; END IF;
  IF NEW.housekeeping IS DISTINCT FROM OLD.housekeeping AND NOT ((OLD.housekeeping='dirty' AND NEW.housekeeping='clean') OR (OLD.housekeeping='clean' AND NEW.housekeeping IN ('inspected','dirty')) OR (OLD.housekeeping='inspected' AND NEW.housekeeping='dirty'))
  THEN RAISE EXCEPTION 'Room housekeeping transition refused' USING ERRCODE='23514'; END IF;
  IF (NOT NEW.active OR NEW.housekeeping IN ('clean','inspected')) AND EXISTS(SELECT 1 FROM archava_accommodation.stays WHERE tenant_id=NEW.tenant_id AND room_id=NEW.id AND state='in_house')
  THEN RAISE EXCEPTION 'Room is physically occupied' USING ERRCODE='23514'; END IF;
 END IF;
 IF NEW.active IS DISTINCT FROM (SELECT active FROM archava_scheduling.resources WHERE tenant_id=NEW.tenant_id AND id=NEW.id)
 THEN RAISE EXCEPTION 'Room availability differs from shared resource' USING ERRCODE='23514'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER room_guard BEFORE INSERT OR UPDATE ON archava_accommodation.rooms FOR EACH ROW EXECUTE FUNCTION archava_accommodation.guard_room();
CREATE FUNCTION archava_accommodation.guard_stay() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE ready archava_accommodation.rooms; slot archava_scheduling.occupancy; observed timestamptz;
BEGIN
 PERFORM id FROM archava_scheduling.resources WHERE tenant_id=NEW.tenant_id AND id=NEW.room_id AND managed_by='accommodation' FOR UPDATE;
 observed:=clock_timestamp();
 SELECT * INTO ready FROM archava_accommodation.rooms WHERE tenant_id=NEW.tenant_id AND id=NEW.room_id;
 SELECT * INTO slot FROM archava_scheduling.occupancy WHERE tenant_id=NEW.tenant_id AND id=NEW.id;
 IF ready.property_id IS DISTINCT FROM NEW.property_id OR slot.resource_id IS DISTINCT FROM NEW.room_id OR slot.owner_module IS DISTINCT FROM 'accommodation'
 OR NEW.starts_at IS DISTINCT FROM slot.starts_at OR NEW.ends_at IS DISTINCT FROM slot.ends_at OR NEW.expires_at IS DISTINCT FROM slot.expires_at
 OR (NEW.state='held' AND slot.state<>'held') OR (NEW.state IN ('confirmed','in_house') AND slot.state<>'confirmed') OR (NEW.state IN ('checked_out','cancelled') AND slot.state<>'cancelled')
 THEN RAISE EXCEPTION 'Stay occupancy binding refused' USING ERRCODE='23514'; END IF;
 IF TG_OP='UPDATE' THEN
  IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.id IS DISTINCT FROM OLD.id OR NEW.property_id IS DISTINCT FROM OLD.property_id OR NEW.room_id IS DISTINCT FROM OLD.room_id OR NEW.owner_id IS DISTINCT FROM OLD.owner_id
  OR NEW.guest_name IS DISTINCT FROM OLD.guest_name OR NEW.guests IS DISTINCT FROM OLD.guests OR NEW.arrival IS DISTINCT FROM OLD.arrival OR NEW.departure IS DISTINCT FROM OLD.departure
  OR NEW.nights IS DISTINCT FROM OLD.nights OR NEW.nightly_rate IS DISTINCT FROM OLD.nightly_rate OR NEW.rate_revision IS DISTINCT FROM OLD.rate_revision OR NEW.total_estimate IS DISTINCT FROM OLD.total_estimate OR NEW.currency IS DISTINCT FROM OLD.currency
  OR NEW.created_at IS DISTINCT FROM OLD.created_at OR NEW.revision<>OLD.revision+1
  THEN RAISE EXCEPTION 'Stay identity and quote are immutable' USING ERRCODE='23514'; END IF;
  IF NOT ((OLD.state='held' AND NEW.state IN ('confirmed','cancelled')) OR (OLD.state='confirmed' AND NEW.state IN ('in_house','cancelled')) OR (OLD.state='in_house' AND NEW.state='checked_out'))
  OR (OLD.state='held' AND OLD.expires_at<=observed)
  THEN RAISE EXCEPTION 'Stay transition refused' USING ERRCODE='23514'; END IF;
  IF OLD.checked_in_at IS NOT NULL AND NEW.checked_in_at IS DISTINCT FROM OLD.checked_in_at THEN RAISE EXCEPTION 'Check-in timestamp is immutable' USING ERRCODE='23514'; END IF;
  IF NEW.state='confirmed' AND (NEW.ends_at<=observed OR NEW.nights<>(SELECT count(*) FROM archava_accommodation.stay_nights WHERE tenant_id=NEW.tenant_id AND stay_id=NEW.id))
  THEN RAISE EXCEPTION 'Stay quote/time is incomplete' USING ERRCODE='23514'; END IF;
  IF NEW.state='in_house' AND (NOT ready.active OR ready.housekeeping<>'inspected' OR observed<NEW.starts_at OR observed>=NEW.ends_at OR NEW.checked_in_at IS NULL)
  THEN RAISE EXCEPTION 'Room is not ready for check-in' USING ERRCODE='23514'; END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER stay_guard BEFORE INSERT OR UPDATE ON archava_accommodation.stays FOR EACH ROW EXECUTE FUNCTION archava_accommodation.guard_stay();
CREATE FUNCTION archava_accommodation.dirty_after_stay() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF NEW.state IN ('in_house','checked_out') THEN UPDATE archava_accommodation.rooms SET housekeeping='dirty',revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=NEW.tenant_id AND id=NEW.room_id; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER stay_room_dirty AFTER UPDATE ON archava_accommodation.stays FOR EACH ROW EXECUTE FUNCTION archava_accommodation.dirty_after_stay();
CREATE FUNCTION archava_accommodation.guard_night() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF TG_OP<>'INSERT' THEN RAISE EXCEPTION 'Night quote is immutable'; END IF;
 IF NOT EXISTS(SELECT 1 FROM archava_accommodation.stays WHERE tenant_id=NEW.tenant_id AND id=NEW.stay_id AND state='held' AND NEW.night_date>=arrival AND NEW.night_date<departure AND NEW.rate=nightly_rate AND NEW.rate_revision=rate_revision)
 THEN RAISE EXCEPTION 'Night quote does not match stay' USING ERRCODE='23514'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER night_guard BEFORE INSERT OR UPDATE OR DELETE ON archava_accommodation.stay_nights FOR EACH ROW EXECUTE FUNCTION archava_accommodation.guard_night();
CREATE FUNCTION archava_accommodation.prevent_audit_change() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN RAISE EXCEPTION 'Accommodation audit is append-only'; END $$;
CREATE TRIGGER accommodation_audit_immutable BEFORE UPDATE OR DELETE ON archava_accommodation.audit FOR EACH ROW EXECUTE FUNCTION archava_accommodation.prevent_audit_change();
CREATE FUNCTION archava_accommodation.prevent_command_change() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF OLD.receipt IS NOT NULL OR TG_OP='DELETE' OR NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.request_id IS DISTINCT FROM OLD.request_id
 OR NEW.actor_id IS DISTINCT FROM OLD.actor_id OR NEW.fingerprint IS DISTINCT FROM OLD.fingerprint OR NEW.created_at IS DISTINCT FROM OLD.created_at
 THEN RAISE EXCEPTION 'Accommodation command is immutable'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER accommodation_command_immutable BEFORE UPDATE OR DELETE ON archava_accommodation.commands FOR EACH ROW EXECUTE FUNCTION archava_accommodation.prevent_command_change();
ALTER TABLE archava_accommodation.memberships ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_accommodation.memberships FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_accommodation.properties ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_accommodation.properties FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_accommodation.room_types ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_accommodation.room_types FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_accommodation.rooms ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_accommodation.rooms FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_accommodation.stays ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_accommodation.stays FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_accommodation.stay_nights ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_accommodation.stay_nights FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_accommodation.commands ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_accommodation.commands FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_accommodation.audit ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_accommodation.audit FORCE ROW LEVEL SECURITY;
CREATE POLICY accommodation_membership_tenant ON archava_accommodation.memberships USING(tenant_id=current_setting('archava.tenant_id',true)) WITH CHECK(tenant_id=current_setting('archava.tenant_id',true));
CREATE POLICY accommodation_property_tenant ON archava_accommodation.properties USING(tenant_id=current_setting('archava.tenant_id',true)) WITH CHECK(tenant_id=current_setting('archava.tenant_id',true));
CREATE POLICY accommodation_type_tenant ON archava_accommodation.room_types USING(tenant_id=current_setting('archava.tenant_id',true)) WITH CHECK(tenant_id=current_setting('archava.tenant_id',true));
CREATE POLICY accommodation_room_tenant ON archava_accommodation.rooms USING(tenant_id=current_setting('archava.tenant_id',true)) WITH CHECK(tenant_id=current_setting('archava.tenant_id',true));
CREATE POLICY accommodation_stay_tenant ON archava_accommodation.stays USING(tenant_id=current_setting('archava.tenant_id',true)) WITH CHECK(tenant_id=current_setting('archava.tenant_id',true));
CREATE POLICY accommodation_night_tenant ON archava_accommodation.stay_nights USING(tenant_id=current_setting('archava.tenant_id',true)) WITH CHECK(tenant_id=current_setting('archava.tenant_id',true));
CREATE POLICY accommodation_command_tenant ON archava_accommodation.commands USING(tenant_id=current_setting('archava.tenant_id',true)) WITH CHECK(tenant_id=current_setting('archava.tenant_id',true));
CREATE POLICY accommodation_audit_tenant ON archava_accommodation.audit USING(tenant_id=current_setting('archava.tenant_id',true)) WITH CHECK(tenant_id=current_setting('archava.tenant_id',true));
CREATE FUNCTION archava_accommodation.lock_membership(wanted_tenant text,wanted_principal text)
RETURNS SETOF archava_accommodation.memberships LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog AS $$
 SELECT * FROM archava_accommodation.memberships WHERE tenant_id=wanted_tenant AND principal_id=wanted_principal AND wanted_tenant=current_setting('archava.tenant_id',true) FOR SHARE
$$;
REVOKE ALL ON FUNCTION archava_accommodation.lock_membership(text,text) FROM PUBLIC;
CREATE FUNCTION archava_accommodation.lock_property(wanted_tenant text,wanted_id uuid)
RETURNS SETOF archava_accommodation.properties LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog AS $$
 SELECT * FROM archava_accommodation.properties WHERE tenant_id=wanted_tenant AND id=wanted_id AND wanted_tenant=current_setting('archava.tenant_id',true) FOR SHARE
$$;
REVOKE ALL ON FUNCTION archava_accommodation.lock_property(text,uuid) FROM PUBLIC;
