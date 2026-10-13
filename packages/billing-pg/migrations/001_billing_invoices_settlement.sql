CREATE TABLE archava_billing.memberships (
  tenant_id text NOT NULL,
  principal_id text NOT NULL,
  display_name text NOT NULL DEFAULT '' CHECK(length(display_name) <= 254),
  kind text NOT NULL CHECK(kind IN ('human', 'service')),
  permissions text[] NOT NULL CHECK(permissions <@ ARRAY['billing.read', 'billing.write', 'billing.settle', 'audit.read']::text[]),
  scope text NOT NULL CHECK(scope IN ('all', 'assigned')),
  active boolean NOT NULL DEFAULT true,
  PRIMARY KEY(tenant_id, principal_id)
);

CREATE TABLE archava_billing.invoices (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  invoice_code text NOT NULL CHECK(length(invoice_code) BETWEEN 1 AND 50),
  customer_ref text NOT NULL CHECK(length(customer_ref) BETWEEN 1 AND 120),
  currency text NOT NULL CHECK(currency ~ '^[A-Z]{3}$'),
  status text NOT NULL DEFAULT 'draft' CHECK(status IN ('draft', 'issued', 'settled', 'void')),
  gross_minor bigint NOT NULL DEFAULT 0 CHECK(gross_minor >= 0),
  correction_minor bigint NOT NULL DEFAULT 0,
  settled_minor bigint NOT NULL DEFAULT 0 CHECK(settled_minor >= 0),
  outstanding_minor bigint NOT NULL DEFAULT 0,
  consumer_ref text NOT NULL,
  owner_id text NOT NULL,
  revision integer NOT NULL DEFAULT 1 CHECK(revision > 0),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  issued_at timestamptz,
  settled_at timestamptz,
  voided_at timestamptz,
  PRIMARY KEY(tenant_id, id),
  FOREIGN KEY(tenant_id, owner_id) REFERENCES archava_billing.memberships(tenant_id, principal_id),
  CONSTRAINT invoice_code_unique UNIQUE (tenant_id, invoice_code),
  -- What is owed is always the frozen total, adjusted by corrections, less settlement.
  CONSTRAINT invoice_balance CHECK (outstanding_minor = gross_minor + correction_minor - settled_minor),
  -- An invoice is only ever marked settled when nothing remains owed.
  CONSTRAINT invoice_settled_balance CHECK (status <> 'settled' OR outstanding_minor = 0)
);

CREATE INDEX invoice_list ON archava_billing.invoices(tenant_id, status, created_at, id);
CREATE INDEX invoice_code ON archava_billing.invoices(tenant_id, invoice_code);
CREATE INDEX invoice_customer ON archava_billing.invoices(tenant_id, customer_ref, id);

CREATE TABLE archava_billing.invoice_lines (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  invoice_id uuid NOT NULL,
  description text NOT NULL CHECK(length(description) BETWEEN 1 AND 300),
  quantity integer NOT NULL CHECK(quantity > 0),
  unit_minor bigint NOT NULL CHECK(unit_minor >= 0),
  line_total_minor bigint NOT NULL CHECK(line_total_minor >= 0),
  recorded_by text NOT NULL,
  recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, id),
  FOREIGN KEY(tenant_id, invoice_id) REFERENCES archava_billing.invoices(tenant_id, id),
  -- A priced line is arithmetically closed: the total cannot drift from its parts.
  CONSTRAINT line_total_arithmetic CHECK (line_total_minor = quantity::bigint * unit_minor)
);

CREATE INDEX invoice_line_invoice ON archava_billing.invoice_lines(tenant_id, invoice_id, recorded_at, id);

CREATE TABLE archava_billing.invoice_corrections (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  invoice_id uuid NOT NULL,
  amount_minor bigint NOT NULL CHECK(amount_minor <> 0),
  reason text NOT NULL CHECK(length(reason) BETWEEN 1 AND 500),
  reverses_id uuid,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  recorded_by text NOT NULL,
  PRIMARY KEY(tenant_id, id),
  FOREIGN KEY(tenant_id, invoice_id) REFERENCES archava_billing.invoices(tenant_id, id)
);

CREATE INDEX invoice_correction_invoice ON archava_billing.invoice_corrections(tenant_id, invoice_id, created_at, id);

CREATE TABLE archava_billing.payment_attempts (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  invoice_id uuid NOT NULL,
  amount_minor bigint NOT NULL CHECK(amount_minor > 0),
  currency text NOT NULL CHECK(currency ~ '^[A-Z]{3}$'),
  provider_ref text CHECK(provider_ref IS NULL OR length(provider_ref) <= 120),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  recorded_by text NOT NULL,
  PRIMARY KEY(tenant_id, id),
  FOREIGN KEY(tenant_id, invoice_id) REFERENCES archava_billing.invoices(tenant_id, id)
);

CREATE INDEX payment_attempt_invoice ON archava_billing.payment_attempts(tenant_id, invoice_id, created_at, id);

CREATE TABLE archava_billing.payment_events (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  invoice_id uuid NOT NULL,
  provider_id text NOT NULL CHECK(length(provider_id) BETWEEN 1 AND 80),
  provider_event_id text NOT NULL CHECK(length(provider_event_id) BETWEEN 1 AND 120),
  outcome text NOT NULL CHECK(outcome IN ('succeeded', 'failed', 'refunded')),
  amount_minor bigint NOT NULL CHECK(amount_minor >= 0),
  currency text NOT NULL CHECK(currency ~ '^[A-Z]{3}$'),
  verified_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  recorded_by text NOT NULL,
  PRIMARY KEY(tenant_id, id),
  FOREIGN KEY(tenant_id, invoice_id) REFERENCES archava_billing.invoices(tenant_id, id),
  -- A provider notification is applied exactly once, however many times it is replayed.
  CONSTRAINT provider_event_unique UNIQUE (tenant_id, provider_event_id)
);

CREATE INDEX payment_event_invoice ON archava_billing.payment_events(tenant_id, invoice_id, verified_at, id);

CREATE TABLE archava_billing.commands (
  tenant_id text NOT NULL,
  request_id uuid NOT NULL,
  actor_id text NOT NULL,
  fingerprint text NOT NULL,
  receipt jsonb,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, request_id),
  FOREIGN KEY(tenant_id, actor_id) REFERENCES archava_billing.memberships(tenant_id, principal_id)
);

CREATE TABLE archava_billing.audit (
  tenant_id text NOT NULL,
  request_id uuid NOT NULL,
  actor_id text NOT NULL,
  operation text NOT NULL,
  invoice_id uuid,
  revision integer NOT NULL CHECK(revision > 0),
  reason text CHECK(reason IS NULL OR length(reason) <= 2000),
  occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, request_id),
  FOREIGN KEY(tenant_id, request_id) REFERENCES archava_billing.commands(tenant_id, request_id)
);

CREATE INDEX billing_audit_history ON archava_billing.audit(tenant_id, invoice_id, revision DESC);

CREATE FUNCTION archava_billing.immutable_evidence() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  RAISE EXCEPTION 'Billing evidence is append-only';
END
$$;

CREATE TRIGGER lines_immutable BEFORE UPDATE OR DELETE ON archava_billing.invoice_lines FOR EACH ROW EXECUTE FUNCTION archava_billing.immutable_evidence();
CREATE TRIGGER corrections_immutable BEFORE UPDATE OR DELETE ON archava_billing.invoice_corrections FOR EACH ROW EXECUTE FUNCTION archava_billing.immutable_evidence();
CREATE TRIGGER attempts_immutable BEFORE UPDATE OR DELETE ON archava_billing.payment_attempts FOR EACH ROW EXECUTE FUNCTION archava_billing.immutable_evidence();
CREATE TRIGGER events_immutable BEFORE UPDATE OR DELETE ON archava_billing.payment_events FOR EACH ROW EXECUTE FUNCTION archava_billing.immutable_evidence();

CREATE FUNCTION archava_billing.immutable_history() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  RAISE EXCEPTION 'Billing audit history is immutable';
END
$$;

CREATE TRIGGER audit_immutable BEFORE UPDATE OR DELETE ON archava_billing.audit FOR EACH ROW EXECUTE FUNCTION archava_billing.immutable_history();

CREATE FUNCTION archava_billing.guard_command() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF TG_OP='DELETE' OR OLD.receipt IS NOT NULL OR NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.request_id IS DISTINCT FROM OLD.request_id OR NEW.actor_id IS DISTINCT FROM OLD.actor_id OR NEW.fingerprint IS DISTINCT FROM OLD.fingerprint OR NEW.created_at IS DISTINCT FROM OLD.created_at THEN
    RAISE EXCEPTION 'Billing command is immutable';
  END IF;
  RETURN NEW;
END
$$;

CREATE TRIGGER command_guard BEFORE UPDATE OR DELETE ON archava_billing.commands FOR EACH ROW EXECUTE FUNCTION archava_billing.guard_command();

-- A priced invoice is retained so its settlement history cannot dangle.
CREATE FUNCTION archava_billing.guard_invoice_retention() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF TG_OP='DELETE' THEN
    RAISE EXCEPTION 'Billing invoices are retained for settlement audit';
  END IF;
  RETURN NEW;
END
$$;

CREATE TRIGGER invoice_retention BEFORE DELETE ON archava_billing.invoices FOR EACH ROW EXECUTE FUNCTION archava_billing.guard_invoice_retention();

-- Status only follows the transitions the domain allows, and a settled invoice is
-- only settled while nothing remains owed. The identity of a priced invoice is
-- fixed the moment it is created, so a snapshot cannot be re-pointed at a
-- different customer, currency or code after the fact. A refund reopens a
-- settled invoice back to issued, which is why settled is not terminal — void is.
CREATE FUNCTION archava_billing.guard_invoice_status() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id
    OR NEW.id IS DISTINCT FROM OLD.id
    OR NEW.invoice_code IS DISTINCT FROM OLD.invoice_code
    OR NEW.customer_ref IS DISTINCT FROM OLD.customer_ref
    OR NEW.currency IS DISTINCT FROM OLD.currency
    OR NEW.consumer_ref IS DISTINCT FROM OLD.consumer_ref
    OR NEW.owner_id IS DISTINCT FROM OLD.owner_id
    OR NEW.created_at IS DISTINCT FROM OLD.created_at
    OR NEW.gross_minor IS DISTINCT FROM OLD.gross_minor THEN
    RAISE EXCEPTION 'Priced invoice identity and total are immutable';
  END IF;

  IF OLD.status = 'void' AND NEW.status IS DISTINCT FROM 'void' THEN
    RAISE EXCEPTION 'A voided invoice is terminal';
  END IF;

  IF NEW.status IS DISTINCT FROM OLD.status AND NOT (
    (OLD.status = 'draft'    AND NEW.status IN ('issued', 'void')) OR
    (OLD.status = 'issued'   AND NEW.status IN ('settled', 'void')) OR
    (OLD.status = 'settled'  AND NEW.status = 'issued')
  ) THEN
    RAISE EXCEPTION 'Invoice cannot move from % to %', OLD.status, NEW.status;
  END IF;

  RETURN NEW;
END
$$;

CREATE TRIGGER invoice_status_guard BEFORE UPDATE ON archava_billing.invoices FOR EACH ROW EXECUTE FUNCTION archava_billing.guard_invoice_status();

DO $$ DECLARE owned text; BEGIN
  FOREACH owned IN ARRAY ARRAY['memberships', 'invoices', 'invoice_lines', 'invoice_corrections', 'payment_attempts', 'payment_events', 'commands', 'audit'] LOOP
    EXECUTE format('ALTER TABLE archava_billing.%I ENABLE ROW LEVEL SECURITY', owned);
    EXECUTE format('ALTER TABLE archava_billing.%I FORCE ROW LEVEL SECURITY', owned);
    EXECUTE format('CREATE POLICY billing_tenant ON archava_billing.%I USING(tenant_id=current_setting(''archava.tenant_id'',true)) WITH CHECK(tenant_id=current_setting(''archava.tenant_id'',true))', owned);
  END LOOP;
END $$;

CREATE FUNCTION archava_billing.lock_membership(wanted_tenant text, wanted_principal text)
RETURNS SETOF archava_billing.memberships LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog AS $$
  SELECT * FROM archava_billing.memberships WHERE tenant_id=wanted_tenant AND principal_id=wanted_principal AND wanted_tenant=current_setting('archava.tenant_id',true) FOR SHARE
$$;

REVOKE ALL ON FUNCTION archava_billing.lock_membership(text, text) FROM PUBLIC;
