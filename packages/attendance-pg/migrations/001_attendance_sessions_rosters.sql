CREATE TABLE archava_attendance.memberships (
  tenant_id text NOT NULL,
  principal_id text NOT NULL,
  display_name text NOT NULL DEFAULT '' CHECK(length(display_name) <= 254),
  kind text NOT NULL CHECK(kind IN ('human', 'service')),
  permissions text[] NOT NULL CHECK(permissions <@ ARRAY['attendance.read', 'attendance.write', 'attendance.checkin', 'audit.read']::text[]),
  scope text NOT NULL CHECK(scope IN ('all', 'assigned')),
  active boolean NOT NULL DEFAULT true,
  PRIMARY KEY(tenant_id, principal_id)
);

CREATE TABLE archava_attendance.sessions (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  title text NOT NULL CHECK(length(title) BETWEEN 1 AND 200),
  session_code text NOT NULL CHECK(length(session_code) BETWEEN 1 AND 50),
  capacity integer CHECK(capacity IS NULL OR capacity > 0),
  start_time timestamptz NOT NULL,
  end_time timestamptz NOT NULL,
  status text NOT NULL DEFAULT 'scheduled' CHECK(status IN ('draft', 'scheduled', 'in_progress', 'completed', 'canceled')),
  location text CHECK(location IS NULL OR length(location) <= 300),
  enrolled_count integer NOT NULL DEFAULT 0 CHECK(enrolled_count >= 0),
  attended_count integer NOT NULL DEFAULT 0 CHECK(attended_count >= 0),
  owner_id text NOT NULL,
  revision integer NOT NULL DEFAULT 1 CHECK(revision > 0),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, id),
  FOREIGN KEY(tenant_id, owner_id) REFERENCES archava_attendance.memberships(tenant_id, principal_id),
  CONSTRAINT session_code_unique UNIQUE (tenant_id, session_code),
  CONSTRAINT session_capacity_limit CHECK (capacity IS NULL OR enrolled_count <= capacity)
);

CREATE INDEX session_list ON archava_attendance.sessions(tenant_id, status, start_time, id);
CREATE INDEX session_code ON archava_attendance.sessions(tenant_id, session_code);
CREATE INDEX session_owner ON archava_attendance.sessions(tenant_id, owner_id, id);

CREATE TABLE archava_attendance.rosters (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  session_id uuid NOT NULL,
  contact_id text NOT NULL CHECK(length(contact_id) <= 80),
  role text NOT NULL DEFAULT 'attendee' CHECK(role IN ('attendee', 'instructor', 'speaker', 'staff')),
  status text NOT NULL DEFAULT 'registered' CHECK(status IN ('registered', 'waitlisted', 'confirmed', 'attended', 'canceled', 'no_show')),
  ticket_code text NOT NULL CHECK(length(ticket_code) BETWEEN 1 AND 100),
  checked_in_at timestamptz,
  check_in_method text CHECK(check_in_method IS NULL OR check_in_method IN ('qr_scan', 'manual_operator', 'kiosk')),
  revision integer NOT NULL DEFAULT 1 CHECK(revision > 0),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, id),
  FOREIGN KEY(tenant_id, session_id) REFERENCES archava_attendance.sessions(tenant_id, id) ON DELETE CASCADE,
  CONSTRAINT roster_session_contact_unique UNIQUE (tenant_id, session_id, contact_id),
  CONSTRAINT roster_ticket_code_unique UNIQUE (tenant_id, ticket_code)
);

CREATE INDEX roster_session ON archava_attendance.rosters(tenant_id, session_id, id);
CREATE INDEX roster_contact ON archava_attendance.rosters(tenant_id, contact_id, id);
CREATE INDEX roster_ticket ON archava_attendance.rosters(tenant_id, ticket_code);

CREATE TABLE archava_attendance.check_in_events (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  roster_id uuid NOT NULL,
  session_id uuid NOT NULL,
  contact_id text NOT NULL CHECK(length(contact_id) <= 80),
  method text NOT NULL CHECK(method IN ('qr_scan', 'manual_operator', 'kiosk')),
  evidence_ref text CHECK(evidence_ref IS NULL OR length(evidence_ref) <= 300),
  idempotency_key text NOT NULL CHECK(length(idempotency_key) BETWEEN 1 AND 100),
  recorded_by text NOT NULL,
  recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, id),
  FOREIGN KEY(tenant_id, roster_id) REFERENCES archava_attendance.rosters(tenant_id, id),
  CONSTRAINT check_in_idempotency UNIQUE (tenant_id, roster_id, idempotency_key)
);

CREATE INDEX checkin_roster ON archava_attendance.check_in_events(tenant_id, roster_id, recorded_at DESC);
CREATE INDEX checkin_session ON archava_attendance.check_in_events(tenant_id, session_id, recorded_at DESC);

CREATE TABLE archava_attendance.commands (
  tenant_id text NOT NULL,
  request_id uuid NOT NULL,
  actor_id text NOT NULL,
  fingerprint text NOT NULL,
  receipt jsonb,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, request_id),
  FOREIGN KEY(tenant_id, actor_id) REFERENCES archava_attendance.memberships(tenant_id, principal_id)
);

CREATE TABLE archava_attendance.audit (
  tenant_id text NOT NULL,
  request_id uuid NOT NULL,
  actor_id text NOT NULL,
  operation text NOT NULL,
  session_id uuid,
  revision integer NOT NULL CHECK(revision > 0),
  reason text CHECK(reason IS NULL OR length(reason) <= 2000),
  occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, request_id),
  FOREIGN KEY(tenant_id, request_id) REFERENCES archava_attendance.commands(tenant_id, request_id)
);

CREATE INDEX attendance_audit_history ON archava_attendance.audit(tenant_id, session_id, revision DESC);

CREATE FUNCTION archava_attendance.immutable_history() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  RAISE EXCEPTION 'Attendance audit history is immutable';
END
$$;

CREATE TRIGGER audit_immutable BEFORE UPDATE OR DELETE ON archava_attendance.audit FOR EACH ROW EXECUTE FUNCTION archava_attendance.immutable_history();

CREATE FUNCTION archava_attendance.guard_command() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF TG_OP='DELETE' OR OLD.receipt IS NOT NULL OR NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.request_id IS DISTINCT FROM OLD.request_id OR NEW.actor_id IS DISTINCT FROM OLD.actor_id OR NEW.fingerprint IS DISTINCT FROM OLD.fingerprint OR NEW.created_at IS DISTINCT FROM OLD.created_at THEN
    RAISE EXCEPTION 'Attendance command is immutable';
  END IF;
  RETURN NEW;
END
$$;

CREATE TRIGGER command_guard BEFORE UPDATE OR DELETE ON archava_attendance.commands FOR EACH ROW EXECUTE FUNCTION archava_attendance.guard_command();

DO $$ DECLARE owned text; BEGIN
  FOREACH owned IN ARRAY ARRAY['memberships', 'sessions', 'rosters', 'check_in_events', 'commands', 'audit'] LOOP
    EXECUTE format('ALTER TABLE archava_attendance.%I ENABLE ROW LEVEL SECURITY', owned);
    EXECUTE format('ALTER TABLE archava_attendance.%I FORCE ROW LEVEL SECURITY', owned);
    EXECUTE format('CREATE POLICY attendance_tenant ON archava_attendance.%I USING(tenant_id=current_setting(''archava.tenant_id'',true)) WITH CHECK(tenant_id=current_setting(''archava.tenant_id'',true))', owned);
  END LOOP;
END $$;

CREATE FUNCTION archava_attendance.lock_membership(wanted_tenant text, wanted_principal text)
RETURNS SETOF archava_attendance.memberships LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog AS $$
  SELECT * FROM archava_attendance.memberships WHERE tenant_id=wanted_tenant AND principal_id=wanted_principal AND wanted_tenant=current_setting('archava.tenant_id',true) FOR SHARE
$$;

REVOKE ALL ON FUNCTION archava_attendance.lock_membership(text, text) FROM PUBLIC;
