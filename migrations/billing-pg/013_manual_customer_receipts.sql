-- Manual customer receipts. A person with billing.settle attests that money
-- for an invoice was received by bank transfer or in cash, or reverses all or
-- part of one such receipt. Both are payment events and move the invoice's
-- settlement exactly like a provider event; nothing here is verified by a
-- payment provider, and a manual event is never labelled as verified.
--
-- No table is added and no grant changes. The facts a manual event carries
-- beside the event itself live in five new columns of payment_events, null on
-- every existing row and on every provider event:
--   manual_method      bank_transfer or cash; provider_id is 'manual:'||method
--   received_on        the UTC calendar date the money was received (receipt)
--   reference          the payer's or bank's reference, or the cash receipt
--                      number (receipt)
--   reason             why the receipt or the reversal was recorded
--   reverses_event_id  the manual receipt a reversal reverses
-- The acting person is the event's recorded_by and the time of recording its
-- verified_at (the column keeps its name; for a manual event it is the time
-- the person recorded it). payment_events stays append-only under the
-- events_immutable trigger of 001, so the facts are immutable with their row.
--
-- The guard below runs before every insert. An event whose provider id is in
-- the reserved 'manual:' namespace must carry the manual facts, so the
-- provider-event path cannot write one; an event that carries manual facts
-- must name its own method as provider id (the row check), so a manual event
-- cannot pose as another provider's. A direct insert by the serving role is
-- held to the same rules as the command. Existing rows are not re-examined.
ALTER TABLE archava_billing.payment_events
  ADD COLUMN manual_method text CHECK(manual_method IN ('bank_transfer','cash')),
  ADD COLUMN received_on date,
  ADD COLUMN reference text CHECK(length(reference) BETWEEN 1 AND 100),
  ADD COLUMN reason text CHECK(length(reason) BETWEEN 1 AND 500),
  ADD COLUMN reverses_event_id uuid,
  ADD CONSTRAINT payment_event_reverses FOREIGN KEY(tenant_id,reverses_event_id) REFERENCES archava_billing.payment_events(tenant_id,id),
  ADD CONSTRAINT payment_event_manual_facts CHECK(
    (manual_method IS NULL AND received_on IS NULL AND reference IS NULL AND reason IS NULL AND reverses_event_id IS NULL)
    OR (manual_method IS NOT NULL AND provider_id='manual:'||manual_method AND provider_event_id='manual:'||id::text
      AND reason IS NOT NULL AND amount_minor>0
      AND ((outcome='succeeded' AND received_on IS NOT NULL AND reference IS NOT NULL AND reverses_event_id IS NULL)
        OR (outcome='refunded' AND received_on IS NULL AND reference IS NULL AND reverses_event_id IS NOT NULL))));
CREATE INDEX payment_event_reversals ON archava_billing.payment_events(tenant_id,reverses_event_id) WHERE reverses_event_id IS NOT NULL;

CREATE FUNCTION archava_billing.guard_manual_receipt() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE i archava_billing.invoices;m archava_billing.memberships;o archava_billing.payment_events;reversed bigint;
BEGIN
 IF NEW.manual_method IS NULL THEN
  IF NEW.provider_id LIKE 'manual:%' THEN
   RAISE EXCEPTION 'A manual payment event needs its method, reason and recording person' USING ERRCODE='23514';END IF;
  RETURN NEW;
 END IF;
 -- The invoice row is locked: receipts and reversals of one invoice are serial.
 SELECT * INTO i FROM archava_billing.invoices WHERE tenant_id=NEW.tenant_id AND id=NEW.invoice_id FOR UPDATE;
 SELECT * INTO m FROM archava_billing.memberships WHERE tenant_id=NEW.tenant_id AND principal_id=NEW.recorded_by;
 -- Recorded by an active person who may settle, in the invoice's currency, at
 -- the clock of its own transaction (the tolerance of Billing 012).
 IF i.id IS NULL OR m.principal_id IS NULL OR m.kind<>'human' OR NOT m.active OR NOT m.permissions @> ARRAY['billing.settle']::text[]
  OR NEW.currency IS DISTINCT FROM i.currency
  OR (NEW.verified_at BETWEEN transaction_timestamp()-interval '1 second' AND clock_timestamp()+interval '1 second') IS NOT TRUE THEN
  RAISE EXCEPTION 'A manual payment event needs its method, reason and recording person' USING ERRCODE='23514';END IF;
 IF NEW.outcome='succeeded' THEN
  -- The command settles the invoice before it stores the event, so an invoice
  -- that took the receipt is issued or settled and owes nothing below zero.
  IF i.status NOT IN ('issued','settled') THEN
   RAISE EXCEPTION 'A manual receipt can be recorded only on an issued invoice' USING ERRCODE='23514';END IF;
  IF i.outstanding_minor<0 THEN
   RAISE EXCEPTION 'A manual receipt cannot exceed the outstanding amount of its invoice' USING ERRCODE='23514';END IF;
  IF NEW.received_on>(NEW.verified_at AT TIME ZONE 'UTC')::date THEN
   RAISE EXCEPTION 'A manual receipt cannot be dated in the future' USING ERRCODE='23514';END IF;
  IF i.issued_at IS NULL OR NEW.received_on<(i.issued_at AT TIME ZONE 'UTC')::date THEN
   RAISE EXCEPTION 'A manual receipt cannot be dated before its invoice was issued' USING ERRCODE='23514';END IF;
  -- The customer could still pay the frozen amount online: both would settle.
  IF EXISTS(SELECT 1 FROM archava_billing.gateway_links WHERE tenant_id=NEW.tenant_id AND invoice_id=NEW.invoice_id AND closed_at IS NULL AND received_minor=0) THEN
   RAISE EXCEPTION 'Close the open online payment before recording a manual receipt' USING ERRCODE='23514';END IF;
 ELSE
  SELECT * INTO o FROM archava_billing.payment_events WHERE tenant_id=NEW.tenant_id AND id=NEW.reverses_event_id;
  IF o.id IS NULL OR o.invoice_id IS DISTINCT FROM NEW.invoice_id OR o.manual_method IS DISTINCT FROM NEW.manual_method OR o.outcome<>'succeeded' THEN
   RAISE EXCEPTION 'Only a manual receipt of the same invoice can be reversed' USING ERRCODE='23514';END IF;
  SELECT coalesce(sum(amount_minor),0) INTO reversed FROM archava_billing.payment_events WHERE tenant_id=NEW.tenant_id AND reverses_event_id=NEW.reverses_event_id;
  IF reversed+NEW.amount_minor>o.amount_minor THEN
   RAISE EXCEPTION 'A manual receipt reversal cannot exceed the unreversed amount of its receipt' USING ERRCODE='23514';END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER payment_event_manual_guard BEFORE INSERT ON archava_billing.payment_events FOR EACH ROW EXECUTE FUNCTION archava_billing.guard_manual_receipt();
