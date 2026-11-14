-- Manual customer receipts, Accounting part (Billing 013 migrates before it).
-- No table, column or row changes. Every replaced function keeps its
-- signature, so the triggers that call it and the readiness functions that
-- identify them by function OID are unchanged.
--
-- 1. invoice_effect() of 014 is replaced in place: a record_manual_receipt
--    receipt is a collection of its event's amount and a
--    reverse_manual_receipt receipt a refund of it, exactly like a succeeded
--    or refunded provider event. The event must carry the manual facts of
--    Billing 013 and the outcome its action implies; a provider event must
--    carry none. A manual command without its event row is refused as a
--    missing effect, not skipped.
-- 2. invoice_effect_date() is new: the source date of a billing command. A
--    manual receipt is dated the day the money was received (received_on of
--    its event), so a transfer received yesterday and entered today is dated
--    yesterday and can be settled against yesterday's bank row. Every other
--    command, a reversal included, keeps the UTC calendar date of the command.
-- 3. guard_invoice_source() of 019 and capture_invoice_source() of 017 are
--    replaced in place and take the source date from that function instead of
--    the command's clock. The closed-period refusal of the guard is unchanged
--    and now reads the received-on date of a manual receipt, so a receipt
--    dated in a closed period is refused with its billing command. The guard
--    also refuses a source dated before its invoice's issue source; Billing
--    013 already refuses a receipt dated before the invoice was issued.
--    guard_journal_source(), the clearing settlement functions of 010 and the
--    subledger reads look only at the source's effect, amount and date and are
--    not replaced. invoice_effect_tax() of 017 answers 0 for both new actions:
--    a collection or refund carries no tax.
-- 4. receipt_guards_complete() is readiness over the two Billing triggers on
--    payment_events that the manual facts depend on (the insert guard of
--    Billing 013 and the append-only trigger of Billing 001), in the shape of
--    tax_document_guards_complete() of 019. The runtime role needs EXECUTE.
CREATE OR REPLACE FUNCTION archava_accounting.invoice_effect(wanted_tenant text,wanted_request uuid) RETURNS TABLE(effect text,whole_idr numeric)
LANGUAGE plpgsql VOLATILE SET search_path=pg_catalog AS $$
DECLARE r jsonb;delta bigint;outcome text;
BEGIN
 SELECT receipt INTO r FROM archava_billing.commands WHERE tenant_id=wanted_tenant AND request_id=wanted_request;
 CASE r->>'action'
  WHEN 'issue_invoice' THEN effect:='issue';whole_idr:=(r->>'outstandingMinor')::numeric;
  WHEN 'append_correction' THEN
   SELECT amount_minor INTO delta FROM archava_billing.invoice_corrections WHERE tenant_id=wanted_tenant AND id=(r->>'correctionId')::uuid AND invoice_id=(r->>'invoiceId')::uuid;
   effect:='correction';whole_idr:=delta;
  WHEN 'void_invoice' THEN effect:='void';whole_idr:=-(r->>'outstandingMinor')::numeric;
  WHEN 'record_provider_event' THEN
   SELECT e.amount_minor,e.outcome INTO delta,outcome FROM archava_billing.payment_events e JOIN archava_billing.invoices i ON i.tenant_id=e.tenant_id AND i.id=e.invoice_id
    WHERE e.tenant_id=wanted_tenant AND e.id=(r->>'eventId')::uuid AND e.invoice_id=(r->>'invoiceId')::uuid AND e.currency=i.currency AND e.manual_method IS NULL;
   IF outcome NOT IN ('succeeded','refunded') OR outcome IS NULL THEN RETURN;END IF;
   effect:=CASE WHEN outcome='succeeded' THEN 'collection' ELSE 'refund' END;
   whole_idr:=CASE WHEN outcome='succeeded' THEN delta ELSE -delta END;
  WHEN 'record_manual_receipt','reverse_manual_receipt' THEN
   SELECT e.amount_minor,e.outcome INTO delta,outcome FROM archava_billing.payment_events e JOIN archava_billing.invoices i ON i.tenant_id=e.tenant_id AND i.id=e.invoice_id
    WHERE e.tenant_id=wanted_tenant AND e.id=(r->>'eventId')::uuid AND e.invoice_id=(r->>'invoiceId')::uuid AND e.currency=i.currency AND e.manual_method IS NOT NULL
     AND e.outcome=CASE r->>'action' WHEN 'record_manual_receipt' THEN 'succeeded' ELSE 'refunded' END;
   effect:=CASE WHEN r->>'action'='record_manual_receipt' THEN 'collection' ELSE 'refund' END;
   whole_idr:=CASE WHEN outcome='succeeded' THEN delta ELSE -delta END;
  ELSE RETURN;
 END CASE;
 IF whole_idr IS NULL THEN RAISE EXCEPTION 'Missing immutable invoice effect' USING ERRCODE='23514';END IF;
 RETURN NEXT;
END $$;

CREATE FUNCTION archava_accounting.invoice_effect_date(wanted_tenant text,wanted_request uuid) RETURNS date
LANGUAGE plpgsql VOLATILE SET search_path=pg_catalog AS $$
DECLARE r jsonb;received date;
BEGIN
 SELECT receipt INTO r FROM archava_billing.commands WHERE tenant_id=wanted_tenant AND request_id=wanted_request;
 IF r->>'action'='record_manual_receipt' THEN
  SELECT e.received_on INTO received FROM archava_billing.payment_events e WHERE e.tenant_id=wanted_tenant AND e.id=(r->>'eventId')::uuid AND e.invoice_id=(r->>'invoiceId')::uuid AND e.manual_method IS NOT NULL;
  RETURN received;
 END IF;
 RETURN ((r->>'executedAt')::timestamptz AT TIME ZONE 'UTC')::date;
END $$;

CREATE OR REPLACE FUNCTION archava_accounting.guard_invoice_source() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE c archava_billing.commands;i archava_billing.invoices;b archava_accounting.books;expected record;held jsonb;original archava_accounting.invoice_sources;unit_factor numeric;
 tax_part numeric;t archava_accounting.tax_codes;a archava_accounting.accounts;
BEGIN
 SELECT * INTO b FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 SELECT * INTO c FROM archava_billing.commands WHERE tenant_id=NEW.tenant_id AND request_id=NEW.id;
 SELECT * INTO i FROM archava_billing.invoices WHERE tenant_id=NEW.tenant_id AND id=NEW.invoice_id;
 SELECT * INTO expected FROM archava_accounting.invoice_effect(NEW.tenant_id,NEW.id);
 SELECT snapshot->'record' INTO held FROM archava_accounting.audit WHERE tenant_id=NEW.tenant_id AND resource_kind='invoice_policy' AND resource_id=NEW.policy_id AND revision=NEW.policy_revision;
 unit_factor:=CASE WHEN held->>'source_unit'='whole_idr' AND b.currency='IDR' AND b.minor_units=2 THEN 100 WHEN held->>'source_unit'='minor' AND b.currency<>'IDR' THEN 1 END;
 IF b.id IS NULL OR b.source_authority<>'native' OR unit_factor IS NULL OR i.id IS NULL OR i.company_id IS DISTINCT FROM b.company_id OR i.currency IS DISTINCT FROM b.currency
  OR c.actor_id IS DISTINCT FROM NEW.actor_id OR c.receipt->>'invoiceId' IS DISTINCT FROM NEW.invoice_id::text OR c.receipt->>'revision' IS DISTINCT FROM NEW.invoice_revision::text
  OR c.receipt->>'executedAt' IS NULL OR (c.receipt->>'executedAt')::timestamptz IS DISTINCT FROM NEW.occurred_at OR NEW.source_date IS DISTINCT FROM archava_accounting.invoice_effect_date(NEW.tenant_id,NEW.id)
  OR NEW.invoice_code IS DISTINCT FROM i.invoice_code OR expected.effect IS DISTINCT FROM NEW.effect OR expected.whole_idr*unit_factor IS DISTINCT FROM NEW.amount_minor::numeric
  OR held IS NULL OR held IS DISTINCT FROM NEW.policy_snapshot OR held->>'book_id' IS DISTINCT FROM NEW.book_id::text THEN
  RAISE EXCEPTION 'Financial source differs from its immutable invoice receipt/policy' USING ERRCODE='23514';END IF;
 IF NEW.effect<>'issue' THEN
  SELECT * INTO original FROM archava_accounting.invoice_sources WHERE tenant_id=NEW.tenant_id AND invoice_id=NEW.invoice_id AND effect='issue';
  IF original.id IS NULL OR (NEW.book_id,NEW.policy_id,NEW.policy_revision,NEW.policy_snapshot) IS DISTINCT FROM (original.book_id,original.policy_id,original.policy_revision,original.policy_snapshot) THEN
   RAISE EXCEPTION 'Invoice corrections retain their original posting policy' USING ERRCODE='23514';END IF;
  -- Money is not received before the invoice it pays exists in the book.
  IF NEW.source_date<original.source_date THEN
   RAISE EXCEPTION 'Financial source differs from its immutable invoice receipt/policy' USING ERRCODE='23514';END IF;
 END IF;
 tax_part:=archava_accounting.invoice_effect_tax(NEW.tenant_id,NEW.id);
 IF tax_part IS NULL OR tax_part*unit_factor IS DISTINCT FROM NEW.tax_minor::numeric THEN
  RAISE EXCEPTION 'Financial source tax differs from the tax stored on its invoice' USING ERRCODE='23514';END IF;
 IF i.tax_code_id IS NULL OR NEW.effect IN ('collection','refund') THEN
  IF NEW.tax_account_id IS NOT NULL OR NEW.tax_account_revision IS NOT NULL THEN
   RAISE EXCEPTION 'Financial source tax differs from the tax stored on its invoice' USING ERRCODE='23514';END IF;
 ELSIF NEW.effect='issue' THEN
  SELECT * INTO t FROM archava_accounting.tax_codes WHERE tenant_id=NEW.tenant_id AND id=i.tax_code_id FOR SHARE;
  IF t.id IS NULL OR t.book_id IS DISTINCT FROM NEW.book_id OR t.kind<>'output' OR t.state NOT IN ('confirmed','retired') THEN
   RAISE EXCEPTION 'The invoice tax code is not a confirmed output tax code of the posting book' USING ERRCODE='23514';END IF;
  SELECT * INTO a FROM archava_accounting.accounts WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND id=t.control_account_id FOR SHARE;
  IF a.id IS NULL OR NOT a.active THEN
   RAISE EXCEPTION 'The tax control account of the invoice tax code is inactive' USING ERRCODE='23514';END IF;
  IF EXISTS(SELECT 1 FROM jsonb_array_elements(held->'mappings') v WHERE (v->>'accountId')::uuid=t.control_account_id) THEN
   RAISE EXCEPTION 'The tax control account is also an account of the invoice posting policy' USING ERRCODE='23514';END IF;
  IF (NEW.tax_account_id,NEW.tax_account_revision) IS DISTINCT FROM (a.id,a.revision) THEN
   RAISE EXCEPTION 'Financial source tax differs from the tax stored on its invoice' USING ERRCODE='23514';END IF;
 ELSIF (NEW.tax_account_id,NEW.tax_account_revision) IS DISTINCT FROM (original.tax_account_id,original.tax_account_revision) THEN
  RAISE EXCEPTION 'Invoice corrections retain their original posting policy' USING ERRCODE='23514';
 END IF;
 IF EXISTS(SELECT 1 FROM archava_accounting.periods WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND state='closed' AND NEW.source_date BETWEEN start_date AND end_date) THEN
  RAISE EXCEPTION 'Reopen the financial source period before changing its invoice' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION archava_accounting.capture_invoice_source() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE i archava_billing.invoices;b archava_accounting.books;p archava_accounting.invoice_policies;original archava_accounting.invoice_sources;expected record;held jsonb;unit_factor numeric;
 tax_account uuid;tax_revision integer;
BEGIN
 IF OLD.receipt IS NOT NULL OR NEW.receipt IS NULL OR NEW.receipt->>'invoiceId' IS NULL THEN RETURN NEW;END IF;
 SELECT * INTO expected FROM archava_accounting.invoice_effect(NEW.tenant_id,NEW.request_id);
 IF expected.effect IS NULL THEN RETURN NEW;END IF;
 SELECT * INTO i FROM archava_billing.invoices WHERE tenant_id=NEW.tenant_id AND id=(NEW.receipt->>'invoiceId')::uuid;
 IF i.company_id IS NULL THEN RETURN NEW;END IF;
 IF expected.effect='issue' THEN
  SELECT * INTO b FROM archava_accounting.books WHERE tenant_id=i.tenant_id AND company_id=i.company_id FOR UPDATE;
  IF b.id IS NULL THEN RETURN NEW;END IF;
  SELECT * INTO p FROM archava_accounting.invoice_policies WHERE tenant_id=i.tenant_id AND book_id=b.id;
  IF p.id IS NULL THEN RETURN NEW;END IF;
  held:=to_jsonb(p);
 ELSE
  SELECT * INTO original FROM archava_accounting.invoice_sources WHERE tenant_id=i.tenant_id AND invoice_id=i.id AND effect='issue';
  -- Historic issued invoices are never silently imported by enabling a policy.
  IF original.id IS NULL THEN RETURN NEW;END IF;
  SELECT * INTO b FROM archava_accounting.books WHERE tenant_id=i.tenant_id AND id=original.book_id FOR UPDATE;
  p.id:=original.policy_id;p.revision:=original.policy_revision;held:=original.policy_snapshot;
 END IF;
 IF i.currency IS DISTINCT FROM b.currency THEN RAISE EXCEPTION 'Invoice currency differs from its company book' USING ERRCODE='23514';END IF;
 unit_factor:=CASE held->>'source_unit' WHEN 'whole_idr' THEN 100 WHEN 'minor' THEN 1 END;
 IF unit_factor IS NULL THEN RAISE EXCEPTION 'Invoice posting policy has no reviewed source unit' USING ERRCODE='23514';END IF;
 IF i.tax_code_id IS NOT NULL AND expected.effect NOT IN ('collection','refund') THEN
  IF expected.effect='issue' THEN
   SELECT t.control_account_id,a.revision INTO tax_account,tax_revision FROM archava_accounting.tax_codes t
    JOIN archava_accounting.accounts a ON a.tenant_id=t.tenant_id AND a.book_id=t.book_id AND a.id=t.control_account_id
    WHERE t.tenant_id=i.tenant_id AND t.book_id=b.id AND t.id=i.tax_code_id;
  ELSE tax_account:=original.tax_account_id;tax_revision:=original.tax_account_revision;
  END IF;
 END IF;
 INSERT INTO archava_accounting.invoice_sources(tenant_id,id,book_id,policy_id,policy_revision,policy_snapshot,invoice_id,invoice_code,invoice_revision,effect,amount_minor,actor_id,occurred_at,source_date,tax_minor,tax_account_id,tax_account_revision)
 VALUES(NEW.tenant_id,NEW.request_id,b.id,p.id,p.revision,held,i.id,i.invoice_code,(NEW.receipt->>'revision')::integer,expected.effect,expected.whole_idr*unit_factor,NEW.actor_id,(NEW.receipt->>'executedAt')::timestamptz,archava_accounting.invoice_effect_date(NEW.tenant_id,NEW.request_id),
  archava_accounting.invoice_effect_tax(NEW.tenant_id,NEW.request_id)*unit_factor,tax_account,tax_revision);
 RETURN NEW;
END $$;

-- A row is missing when no trigger of that name on that table runs the
-- expected function with the expected timing and events, no column list, no
-- WHEN clause and no arguments, enabled for ordinary sessions.
--   7 = BEFORE INSERT FOR EACH ROW, 27 = BEFORE UPDATE OR DELETE FOR EACH ROW.
CREATE FUNCTION archava_accounting.receipt_guards_complete() RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog AS $$
 SELECT NOT EXISTS(SELECT 1 FROM (VALUES
 ('archava_billing','payment_events','payment_event_manual_guard','archava_billing.guard_manual_receipt()',7),
 ('archava_billing','payment_events','events_immutable','archava_billing.immutable_evidence()',27)
 ) AS required(schema_name,table_name,trigger_name,function_name,trigger_type)
 WHERE NOT EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid=to_regclass(format('%I.%I',required.schema_name,required.table_name)) AND t.tgname=required.trigger_name
  AND t.tgfoid=to_regprocedure(required.function_name) AND t.tgtype=required.trigger_type AND t.tgenabled='O' AND NOT t.tgisinternal
  AND NOT t.tgdeferrable AND NOT t.tginitdeferred AND t.tgqual IS NULL
  AND t.tgattr=''::int2vector AND t.tgnargs=0))
$$;
REVOKE ALL ON FUNCTION archava_accounting.receipt_guards_complete() FROM PUBLIC;
