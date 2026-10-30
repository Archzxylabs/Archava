-- Native gateway correlations. External settlement is never inferred from a
-- browser redirect or a notification's unsigned transaction_status field.
-- Global operational latch, analogous to the serving-writer lease. Runtime
-- has SELECT only; restore sets held=true before reopening the isolated target.
CREATE TABLE archava_billing.gateway_runtime (
  singleton boolean PRIMARY KEY DEFAULT true CHECK(singleton),
  held boolean NOT NULL DEFAULT false
);
INSERT INTO archava_billing.gateway_runtime(singleton,held) VALUES(true,false);
ALTER TABLE archava_billing.gateway_runtime ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_billing.gateway_runtime FORCE ROW LEVEL SECURITY;
CREATE POLICY gateway_runtime_read ON archava_billing.gateway_runtime USING(true) WITH CHECK(true);

CREATE TABLE archava_billing.gateway_links (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  invoice_id uuid NOT NULL,
  integration_id text NOT NULL CHECK(integration_id ~ '^[a-zA-Z0-9_-]{1,64}$'),
  service_id text NOT NULL,
  actor_id text NOT NULL,
  request_id uuid NOT NULL,
  order_id text NOT NULL CHECK(order_id ~ '^archava-[a-f0-9-]{36}$'),
  amount_minor bigint NOT NULL CHECK(amount_minor BETWEEN 1 AND 9007199254740991),
  invoice_revision integer NOT NULL CHECK(invoice_revision > 0),
  state text NOT NULL CHECK(state IN ('creating','creation_unknown','open','pending','failed','settled','refunded','review')),
  redirect_url text CHECK(length(redirect_url) BETWEEN 1 AND 4096),
  transaction_id text CHECK(length(transaction_id) BETWEEN 1 AND 120),
  received_minor bigint NOT NULL DEFAULT 0 CHECK(received_minor >= 0),
  refunded_minor bigint NOT NULL DEFAULT 0 CHECK(refunded_minor >= 0),
  allocated_minor bigint NOT NULL DEFAULT 0 CHECK(allocated_minor BETWEEN 0 AND 9007199254740991),
  allocation_refunded_minor bigint NOT NULL DEFAULT 0 CHECK(allocation_refunded_minor >= 0),
  requires_review boolean NOT NULL DEFAULT false,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  last_checked_at timestamptz,
  closed_at timestamptz,
  PRIMARY KEY(tenant_id,id),
  UNIQUE(tenant_id,request_id),
  UNIQUE(tenant_id,order_id),
  FOREIGN KEY(tenant_id,invoice_id) REFERENCES archava_billing.invoices(tenant_id,id),
  FOREIGN KEY(tenant_id,actor_id) REFERENCES archava_billing.memberships(tenant_id,principal_id),
  FOREIGN KEY(tenant_id,service_id) REFERENCES archava_billing.memberships(tenant_id,principal_id),
  CHECK(received_minor IN (0,amount_minor)),
  CHECK(refunded_minor <= received_minor),
  CHECK(allocation_refunded_minor <= allocated_minor)
);
CREATE UNIQUE INDEX gateway_one_active_invoice ON archava_billing.gateway_links(tenant_id,invoice_id) WHERE closed_at IS NULL;
CREATE INDEX gateway_invoice_history ON archava_billing.gateway_links(tenant_id,invoice_id,created_at DESC,id);

CREATE TABLE archava_billing.gateway_observations (
  tenant_id text NOT NULL,
  link_id uuid NOT NULL,
  fingerprint text NOT NULL CHECK(length(fingerprint)=64),
  observation jsonb NOT NULL,
  recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id,link_id,fingerprint),
  FOREIGN KEY(tenant_id,link_id) REFERENCES archava_billing.gateway_links(tenant_id,id)
);
CREATE TRIGGER gateway_observation_retention BEFORE UPDATE OR DELETE ON archava_billing.gateway_observations FOR EACH ROW EXECUTE FUNCTION archava_billing.immutable_evidence();

CREATE FUNCTION archava_billing.guard_gateway_link() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF TG_OP='DELETE' OR (NEW.tenant_id,NEW.id,NEW.invoice_id,NEW.integration_id,NEW.service_id,NEW.actor_id,NEW.request_id,NEW.order_id,NEW.amount_minor,NEW.invoice_revision,NEW.created_at)
    IS DISTINCT FROM (OLD.tenant_id,OLD.id,OLD.invoice_id,OLD.integration_id,OLD.service_id,OLD.actor_id,OLD.request_id,OLD.order_id,OLD.amount_minor,OLD.invoice_revision,OLD.created_at)
    OR (OLD.transaction_id IS NOT NULL AND NEW.transaction_id IS DISTINCT FROM OLD.transaction_id)
    OR (OLD.redirect_url IS NOT NULL AND NEW.redirect_url IS DISTINCT FROM OLD.redirect_url)
    OR NEW.received_minor < OLD.received_minor OR NEW.refunded_minor < OLD.refunded_minor
    OR NEW.allocated_minor < OLD.allocated_minor OR NEW.allocation_refunded_minor < OLD.allocation_refunded_minor
    OR (OLD.closed_at IS NOT NULL AND NEW.closed_at IS DISTINCT FROM OLD.closed_at) THEN
    RAISE EXCEPTION 'Gateway identity and cumulative money are retained';
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER gateway_link_guard BEFORE UPDATE OR DELETE ON archava_billing.gateway_links FOR EACH ROW EXECUTE FUNCTION archava_billing.guard_gateway_link();

-- Hold financial edits while a customer might still pay the frozen amount.
-- Settlement/refund updates themselves do not change correction or void state.
CREATE FUNCTION archava_billing.guard_gateway_invoice() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF (NEW.correction_minor IS DISTINCT FROM OLD.correction_minor OR NEW.status='void')
    AND EXISTS(SELECT 1 FROM archava_billing.gateway_links WHERE tenant_id=OLD.tenant_id AND invoice_id=OLD.id AND closed_at IS NULL AND received_minor=0) THEN
    RAISE EXCEPTION 'Resolve the active payment link before changing its invoice';
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER gateway_invoice_guard BEFORE UPDATE ON archava_billing.invoices FOR EACH ROW EXECUTE FUNCTION archava_billing.guard_gateway_invoice();

DO $$ DECLARE owned text; BEGIN
  FOREACH owned IN ARRAY ARRAY['gateway_links','gateway_observations'] LOOP
    EXECUTE format('ALTER TABLE archava_billing.%I ENABLE ROW LEVEL SECURITY',owned);
    EXECUTE format('ALTER TABLE archava_billing.%I FORCE ROW LEVEL SECURITY',owned);
    EXECUTE format('CREATE POLICY billing_tenant ON archava_billing.%I USING(tenant_id=current_setting(''archava.tenant_id'',true)) WITH CHECK(tenant_id=current_setting(''archava.tenant_id'',true))',owned);
  END LOOP;
END $$;
