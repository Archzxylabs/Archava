CREATE TABLE archava_laundry.memberships (
  tenant_id text NOT NULL,
  principal_id text NOT NULL,
  display_name text NOT NULL DEFAULT '' CHECK(length(display_name) <= 254),
  kind text NOT NULL CHECK(kind IN ('human', 'service')),
  permissions text[] NOT NULL CHECK(permissions <@ ARRAY['laundry.read', 'laundry.write', 'laundry.process', 'audit.read']::text[]),
  scope text NOT NULL CHECK(scope IN ('all', 'assigned')),
  active boolean NOT NULL DEFAULT true,
  PRIMARY KEY(tenant_id, principal_id)
);

CREATE TABLE archava_laundry.orders (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  order_code text NOT NULL CHECK(length(order_code) BETWEEN 1 AND 50),
  kind text NOT NULL CHECK(kind IN ('weight', 'item')),
  customer_ref text CHECK(customer_ref IS NULL OR length(customer_ref) <= 80),
  status text NOT NULL DEFAULT 'received' CHECK(status IN ('received', 'processing', 'ready', 'returned', 'canceled')),
  currency text NOT NULL CHECK(currency ~ '^[A-Z]{3}$'),
  total_minor integer NOT NULL CHECK(total_minor >= 0),
  item_count integer NOT NULL DEFAULT 0 CHECK(item_count >= 0),
  weight_grams integer NOT NULL DEFAULT 0 CHECK(weight_grams >= 0),
  unreconciled_count integer NOT NULL DEFAULT 0 CHECK(unreconciled_count >= 0),
  owner_id text NOT NULL,
  revision integer NOT NULL DEFAULT 1 CHECK(revision > 0),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, id),
  FOREIGN KEY(tenant_id, owner_id) REFERENCES archava_laundry.memberships(tenant_id, principal_id),
  CONSTRAINT order_code_unique UNIQUE (tenant_id, order_code),
  CONSTRAINT order_quality_gate CHECK (status <> 'ready' OR unreconciled_count = 0)
);

CREATE INDEX order_list ON archava_laundry.orders(tenant_id, status, created_at, id);
CREATE INDEX order_code ON archava_laundry.orders(tenant_id, order_code);
CREATE INDEX order_customer_ref ON archava_laundry.orders(tenant_id, customer_ref, id);

CREATE TABLE archava_laundry.order_lines (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  order_id uuid NOT NULL,
  item_ref text NOT NULL CHECK(length(item_ref) BETWEEN 1 AND 80),
  quantity integer NOT NULL CHECK(quantity > 0),
  unit text NOT NULL CHECK(unit IN ('piece', 'gram')),
  unit_price_minor integer NOT NULL CHECK(unit_price_minor >= 0),
  line_total_minor integer NOT NULL CHECK(line_total_minor >= 0),
  recorded_by text NOT NULL,
  recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, id),
  FOREIGN KEY(tenant_id, order_id) REFERENCES archava_laundry.orders(tenant_id, id)
);

CREATE INDEX order_line_order ON archava_laundry.order_lines(tenant_id, order_id, recorded_at, id);

CREATE TABLE archava_laundry.process_events (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  order_id uuid NOT NULL,
  event text NOT NULL CHECK(event IN ('intake', 'tag', 'wash', 'dry', 'press', 'quality_check', 'ready', 'handover')),
  note text CHECK(note IS NULL OR length(note) <= 500),
  evidence_ref text CHECK(evidence_ref IS NULL OR length(evidence_ref) <= 300),
  recorded_by text NOT NULL,
  recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, id),
  FOREIGN KEY(tenant_id, order_id) REFERENCES archava_laundry.orders(tenant_id, id)
);

CREATE INDEX process_order ON archava_laundry.process_events(tenant_id, order_id, recorded_at, id);

CREATE TABLE archava_laundry.findings (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  order_id uuid NOT NULL,
  category text NOT NULL CHECK(category IN ('stain', 'damage', 'missing', 'count')),
  disposition text NOT NULL CHECK(disposition IN ('accepted', 'partial', 'rejected')),
  quantity integer CHECK(quantity IS NULL OR quantity > 0),
  reason text NOT NULL CHECK(length(reason) BETWEEN 1 AND 500),
  evidence_ref text CHECK(evidence_ref IS NULL OR length(evidence_ref) <= 300),
  reconciles_id uuid,
  recorded_by text NOT NULL,
  recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, id),
  FOREIGN KEY(tenant_id, order_id) REFERENCES archava_laundry.orders(tenant_id, id),
  CONSTRAINT finding_reconciles_requires_acceptance CHECK (reconciles_id IS NULL OR disposition = 'accepted')
);

CREATE INDEX finding_order ON archava_laundry.findings(tenant_id, order_id, recorded_at, id);
CREATE UNIQUE INDEX finding_reconciles_once ON archava_laundry.findings(tenant_id, reconciles_id) WHERE reconciles_id IS NOT NULL;

CREATE TABLE archava_laundry.commands (
  tenant_id text NOT NULL,
  request_id uuid NOT NULL,
  actor_id text NOT NULL,
  fingerprint text NOT NULL,
  receipt jsonb,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, request_id),
  FOREIGN KEY(tenant_id, actor_id) REFERENCES archava_laundry.memberships(tenant_id, principal_id)
);

CREATE TABLE archava_laundry.audit (
  tenant_id text NOT NULL,
  request_id uuid NOT NULL,
  actor_id text NOT NULL,
  operation text NOT NULL,
  order_id uuid,
  revision integer NOT NULL CHECK(revision > 0),
  reason text CHECK(reason IS NULL OR length(reason) <= 2000),
  occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, request_id),
  FOREIGN KEY(tenant_id, request_id) REFERENCES archava_laundry.commands(tenant_id, request_id)
);

CREATE INDEX laundry_audit_history ON archava_laundry.audit(tenant_id, order_id, revision DESC);

CREATE FUNCTION archava_laundry.immutable_evidence() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  RAISE EXCEPTION 'Laundry evidence is append-only';
END
$$;

CREATE TRIGGER lines_immutable BEFORE UPDATE OR DELETE ON archava_laundry.order_lines FOR EACH ROW EXECUTE FUNCTION archava_laundry.immutable_evidence();
CREATE TRIGGER process_immutable BEFORE UPDATE OR DELETE ON archava_laundry.process_events FOR EACH ROW EXECUTE FUNCTION archava_laundry.immutable_evidence();
CREATE TRIGGER findings_immutable BEFORE UPDATE OR DELETE ON archava_laundry.findings FOR EACH ROW EXECUTE FUNCTION archava_laundry.immutable_evidence();

CREATE FUNCTION archava_laundry.immutable_history() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  RAISE EXCEPTION 'Laundry audit history is immutable';
END
$$;

CREATE TRIGGER audit_immutable BEFORE UPDATE OR DELETE ON archava_laundry.audit FOR EACH ROW EXECUTE FUNCTION archava_laundry.immutable_history();

CREATE FUNCTION archava_laundry.guard_command() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF TG_OP='DELETE' OR OLD.receipt IS NOT NULL OR NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.request_id IS DISTINCT FROM OLD.request_id OR NEW.actor_id IS DISTINCT FROM OLD.actor_id OR NEW.fingerprint IS DISTINCT FROM OLD.fingerprint OR NEW.created_at IS DISTINCT FROM OLD.created_at THEN
    RAISE EXCEPTION 'Laundry command is immutable';
  END IF;
  RETURN NEW;
END
$$;

CREATE TRIGGER command_guard BEFORE UPDATE OR DELETE ON archava_laundry.commands FOR EACH ROW EXECUTE FUNCTION archava_laundry.guard_command();

-- The priced order intent is frozen at creation; only lifecycle state advances.
CREATE FUNCTION archava_laundry.guard_order_intent() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF TG_OP='DELETE' THEN
    RAISE EXCEPTION 'Laundry orders are retained for custody audit';
  END IF;
  IF NEW.order_code IS DISTINCT FROM OLD.order_code OR NEW.kind IS DISTINCT FROM OLD.kind OR NEW.customer_ref IS DISTINCT FROM OLD.customer_ref OR NEW.currency IS DISTINCT FROM OLD.currency OR NEW.total_minor IS DISTINCT FROM OLD.total_minor OR NEW.created_at IS DISTINCT FROM OLD.created_at OR NEW.owner_id IS DISTINCT FROM OLD.owner_id THEN
    RAISE EXCEPTION 'Laundry order intent is immutable after creation';
  END IF;
  RETURN NEW;
END
$$;

CREATE TRIGGER order_intent_guard BEFORE UPDATE OR DELETE ON archava_laundry.orders FOR EACH ROW EXECUTE FUNCTION archava_laundry.guard_order_intent();

DO $$ DECLARE owned text; BEGIN
  FOREACH owned IN ARRAY ARRAY['memberships', 'orders', 'order_lines', 'process_events', 'findings', 'commands', 'audit'] LOOP
    EXECUTE format('ALTER TABLE archava_laundry.%I ENABLE ROW LEVEL SECURITY', owned);
    EXECUTE format('ALTER TABLE archava_laundry.%I FORCE ROW LEVEL SECURITY', owned);
    EXECUTE format('CREATE POLICY laundry_tenant ON archava_laundry.%I USING(tenant_id=current_setting(''archava.tenant_id'',true)) WITH CHECK(tenant_id=current_setting(''archava.tenant_id'',true))', owned);
  END LOOP;
END $$;

CREATE FUNCTION archava_laundry.lock_membership(wanted_tenant text, wanted_principal text)
RETURNS SETOF archava_laundry.memberships LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog AS $$
  SELECT * FROM archava_laundry.memberships WHERE tenant_id=wanted_tenant AND principal_id=wanted_principal AND wanted_tenant=current_setting('archava.tenant_id',true) FOR SHARE
$$;

REVOKE ALL ON FUNCTION archava_laundry.lock_membership(text, text) FROM PUBLIC;
