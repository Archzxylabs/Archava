-- Transactional email outbox for invoices and statements.
-- Exactly-once attempt correlation: an email intent is committed before transport invocation.

CREATE TABLE archava_billing.email_outbox (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  invoice_id uuid NOT NULL,
  actor_id text NOT NULL,
  request_id uuid NOT NULL,
  recipient text NOT NULL CHECK(length(recipient) BETWEEN 3 AND 320 AND recipient ~ '^[^@\s\r\n]+@[^@\s\r\n]+\.[^@\s\r\n]+$'),
  subject text NOT NULL CHECK(length(subject) BETWEEN 1 AND 256 AND subject !~ '[\r\n]'),
  payload_sha256 text NOT NULL CHECK(length(payload_sha256) = 64),
  state text NOT NULL CHECK(state IN ('queued', 'invoking', 'accepted', 'unknown', 'rejected', 'held')),
  provider text NOT NULL CHECK(provider IN ('resend', 'smtp', 'postmark')),
  provider_message_id text CHECK(length(provider_message_id) BETWEEN 1 AND 256),
  error_reason text CHECK(length(error_reason) <= 512),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, id),
  UNIQUE(tenant_id, request_id),
  FOREIGN KEY(tenant_id, invoice_id) REFERENCES archava_billing.invoices(tenant_id, id),
  FOREIGN KEY(tenant_id, actor_id) REFERENCES archava_billing.memberships(tenant_id, principal_id)
);

CREATE INDEX email_outbox_invoice ON archava_billing.email_outbox(tenant_id, invoice_id, created_at DESC, id);

CREATE FUNCTION archava_billing.guard_email_outbox() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF TG_OP = 'DELETE' THEN
    RAISE EXCEPTION 'Email outbox records are permanent and cannot be deleted';
  END IF;

  IF (NEW.tenant_id, NEW.id, NEW.invoice_id, NEW.actor_id, NEW.request_id, NEW.recipient, NEW.payload_sha256, NEW.created_at)
    IS DISTINCT FROM (OLD.tenant_id, OLD.id, OLD.invoice_id, OLD.actor_id, OLD.request_id, OLD.recipient, OLD.payload_sha256, OLD.created_at) THEN
    RAISE EXCEPTION 'Email outbox identity, recipient, and payload hash are immutable';
  END IF;

  IF OLD.state = 'accepted' AND NEW.state IS DISTINCT FROM 'accepted' THEN
    RAISE EXCEPTION 'Accepted email state cannot revert';
  END IF;

  IF OLD.state = 'rejected' AND NEW.state IS DISTINCT FROM 'rejected' THEN
    RAISE EXCEPTION 'Rejected email state cannot revert';
  END IF;

  NEW.updated_at = clock_timestamp();
  RETURN NEW;
END $$;

CREATE TRIGGER email_outbox_guard BEFORE UPDATE OR DELETE ON archava_billing.email_outbox FOR EACH ROW EXECUTE FUNCTION archava_billing.guard_email_outbox();

ALTER TABLE archava_billing.email_outbox ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_billing.email_outbox FORCE ROW LEVEL SECURITY;
CREATE POLICY billing_tenant ON archava_billing.email_outbox
  USING(tenant_id = current_setting('archava.tenant_id', true))
  WITH CHECK(tenant_id = current_setting('archava.tenant_id', true));
