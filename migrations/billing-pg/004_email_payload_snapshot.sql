-- Immutable provider payload snapshot for uncertain email outcomes.
-- A retry must replay the exact bytes that were originally claimed, even if
-- the invoice is corrected after the provider call became uncertain.

ALTER TABLE archava_billing.email_outbox
  ADD COLUMN payload_text text NOT NULL DEFAULT '',
  ADD COLUMN payload_html text,
  ADD COLUMN from_address text NOT NULL DEFAULT '';

-- Older uncertain rows predate the snapshot columns and cannot be replayed
-- safely. Hold them for operator reconciliation instead of sending an empty
-- or reconstructed message after this migration.
UPDATE archava_billing.email_outbox
SET state = 'held',
    error_reason = 'Pre-snapshot uncertain invocation requires reconciliation'
WHERE state IN ('invoking', 'unknown');

CREATE OR REPLACE FUNCTION archava_billing.guard_email_outbox() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF TG_OP = 'DELETE' THEN
    RAISE EXCEPTION 'Email outbox records are permanent and cannot be deleted';
  END IF;

  IF (NEW.tenant_id, NEW.id, NEW.invoice_id, NEW.actor_id, NEW.request_id,
      NEW.recipient, NEW.payload_sha256, NEW.payload_text, NEW.payload_html,
      NEW.from_address, NEW.created_at)
    IS DISTINCT FROM
     (OLD.tenant_id, OLD.id, OLD.invoice_id, OLD.actor_id, OLD.request_id,
      OLD.recipient, OLD.payload_sha256, OLD.payload_text, OLD.payload_html,
      OLD.from_address, OLD.created_at) THEN
    RAISE EXCEPTION 'Email outbox identity, recipient, payload, and sender are immutable';
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
