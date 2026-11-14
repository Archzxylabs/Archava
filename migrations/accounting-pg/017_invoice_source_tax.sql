-- Tax stage C: a taxed sales invoice posts its tax to the ledger. A captured
-- invoice source keeps the tax part of its amount (ledger minor units, the same
-- unit factor as the amount) and the tax control account of the tax code version
-- stored on the invoice, with that account's revision at capture. The issue
-- source takes the account from the tax code; a correction or
-- void keeps the account of its issue source, as it keeps the policy. A payment
-- carries no tax. The invoice posting policy and its three roles are unchanged.
-- A source of an untaxed invoice is stored and drafted exactly as before.
-- Earlier SQL bytes are unchanged: the three bridge functions are replaced in
-- place with their signatures, so their OIDs, owners, grants and the triggers
-- source_guards_complete() identifies by OID are kept. Existing rows get tax 0
-- and no tax account; nothing is imported and no row is rewritten.
DO $$ BEGIN
 PERFORM pg_advisory_xact_lock(hashtext('archava-serving-writer-takeover'));
 IF to_regclass('archava_control.writer_lease') IS NOT NULL THEN
  PERFORM 1 FROM archava_control.writer_lease WHERE lease_name='archava-serving' FOR UPDATE;
 END IF;
END $$;
ALTER TABLE archava_accounting.invoice_sources
 ADD COLUMN tax_minor bigint NOT NULL DEFAULT 0,ADD COLUMN tax_account_id uuid,ADD COLUMN tax_account_revision integer,
 ADD CONSTRAINT invoice_source_tax CHECK((
  (tax_account_id IS NULL AND tax_account_revision IS NULL AND tax_minor=0)
  OR (tax_account_id IS NOT NULL AND tax_account_revision>0 AND effect IN ('issue','correction','void')
   AND ((amount_minor>=0 AND tax_minor BETWEEN 0 AND amount_minor) OR (amount_minor<0 AND tax_minor BETWEEN amount_minor AND 0)))) IS TRUE),
 ADD CONSTRAINT invoice_source_tax_account FOREIGN KEY(tenant_id,book_id,tax_account_id) REFERENCES archava_accounting.accounts(tenant_id,book_id,id);

-- invoice_effect returns TABLE(effect,whole_idr) and a function replaced in
-- place cannot change its result type, so the tax part of the same effect is a
-- second function. It reads what Billing stored (Billing 009): the invoice's
-- tax for an issue, the correction row's tax for a correction, the negated tax
-- of the corrected total for a void, and zero for anything else. The amount is
-- in the invoice's source unit; the caller applies the policy's unit factor.
-- NULL means the stored document row is missing.
CREATE FUNCTION archava_accounting.invoice_effect_tax(wanted_tenant text,wanted_request uuid) RETURNS numeric
LANGUAGE plpgsql VOLATILE SET search_path=pg_catalog AS $$
DECLARE r jsonb;part numeric;
BEGIN
 SELECT receipt INTO r FROM archava_billing.commands WHERE tenant_id=wanted_tenant AND request_id=wanted_request;
 CASE r->>'action'
  WHEN 'issue_invoice' THEN
   SELECT i.tax_minor INTO part FROM archava_billing.invoices i WHERE i.tenant_id=wanted_tenant AND i.id=(r->>'invoiceId')::uuid;
  WHEN 'append_correction' THEN
   SELECT c.tax_minor INTO part FROM archava_billing.invoice_corrections c WHERE c.tenant_id=wanted_tenant AND c.id=(r->>'correctionId')::uuid AND c.invoice_id=(r->>'invoiceId')::uuid;
  WHEN 'void_invoice' THEN
   SELECT -(i.tax_minor+i.correction_tax_minor) INTO part FROM archava_billing.invoices i WHERE i.tenant_id=wanted_tenant AND i.id=(r->>'invoiceId')::uuid;
  ELSE part:=0;
 END CASE;
 RETURN part;
END $$;

-- The source guard of 014 with the tax checks added. The captured tax is the
-- document's stored tax for the effect times the unit factor. A source of a
-- taxed invoice names the tax control account: at issue the account of the
-- invoice's tax code version, which must still be a confirmed or retired output
-- code of this book whose account is active and is none of the policy's
-- accounts (an account's identity and class never change, so its revision is
-- not compared: reactivating a deactivated account releases the invoice), with
-- the account's current revision; afterwards the account of the issue source. Applicability on a date is not evaluated again: the
-- invoice's stored snapshot governs.
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
  OR c.receipt->>'executedAt' IS NULL OR (c.receipt->>'executedAt')::timestamptz IS DISTINCT FROM NEW.occurred_at OR NEW.source_date IS DISTINCT FROM (NEW.occurred_at AT TIME ZONE 'UTC')::date
  OR NEW.invoice_code IS DISTINCT FROM i.invoice_code OR expected.effect IS DISTINCT FROM NEW.effect OR expected.whole_idr*unit_factor IS DISTINCT FROM NEW.amount_minor::numeric
  OR held IS NULL OR held IS DISTINCT FROM NEW.policy_snapshot OR held->>'book_id' IS DISTINCT FROM NEW.book_id::text THEN
  RAISE EXCEPTION 'Financial source differs from its immutable invoice receipt/policy' USING ERRCODE='23514';END IF;
 IF NEW.effect<>'issue' THEN
  SELECT * INTO original FROM archava_accounting.invoice_sources WHERE tenant_id=NEW.tenant_id AND invoice_id=NEW.invoice_id AND effect='issue';
  IF original.id IS NULL OR (NEW.book_id,NEW.policy_id,NEW.policy_revision,NEW.policy_snapshot) IS DISTINCT FROM (original.book_id,original.policy_id,original.policy_revision,original.policy_snapshot) THEN
   RAISE EXCEPTION 'Invoice corrections retain their original posting policy' USING ERRCODE='23514';END IF;
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
  IF EXISTS(SELECT 1 FROM jsonb_array_elements(held->'mappings') v WHERE v->>'accountId'=t.control_account_id::text) THEN
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

-- The capture of 014 with the tax part and the tax control account added. It
-- only derives the values; every refusal is the guard's, so a direct insert is
-- held to the same rules. Still quiet without a company, a book or a policy,
-- and for an invoice issued before its policy.
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
 VALUES(NEW.tenant_id,NEW.request_id,b.id,p.id,p.revision,held,i.id,i.invoice_code,(NEW.receipt->>'revision')::integer,expected.effect,expected.whole_idr*unit_factor,NEW.actor_id,(NEW.receipt->>'executedAt')::timestamptz,((NEW.receipt->>'executedAt')::timestamptz AT TIME ZONE 'UTC')::date,
  archava_accounting.invoice_effect_tax(NEW.tenant_id,NEW.request_id)*unit_factor,tax_account,tax_revision);
 RETURN NEW;
END $$;

-- The draft guard of 002. A source without tax needs the same two lines as
-- before. A source with tax needs the receivable line of the captured amount,
-- the tax line of the captured tax on the captured tax control account and,
-- unless the whole amount is tax, the revenue line of the difference: issue is
-- debit receivable, credit revenue and tax; a negative amount is the reverse.
-- A journal line cannot be zero, so a source whose tax is zero has no tax line.
CREATE OR REPLACE FUNCTION archava_accounting.guard_journal_source() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE s archava_accounting.invoice_sources;j archava_accounting.journals;ar uuid;counterpart uuid;debit uuid;credit uuid;net bigint;
BEGIN
 PERFORM 1 FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 SELECT * INTO s FROM archava_accounting.invoice_sources WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND id=NEW.source_id;
 SELECT * INTO j FROM archava_accounting.journals WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND id=NEW.journal_id;
 IF s.id IS NULL OR s.amount_minor=0 OR j.id IS NULL OR j.state<>'draft' OR j.reversal_of IS NOT NULL OR j.entry_date<>s.source_date OR j.source_ref<>'billing:'||s.id::text||':'||j.code
  OR EXISTS(SELECT 1 FROM archava_accounting.journal_sources x JOIN archava_accounting.journals y ON y.tenant_id=x.tenant_id AND y.id=x.journal_id WHERE x.tenant_id=NEW.tenant_id AND x.source_id=NEW.source_id AND y.state<>'void') THEN
  RAISE EXCEPTION 'A financial source needs one reviewed live draft' USING ERRCODE='23514';END IF;
 SELECT (v->>'accountId')::uuid INTO ar FROM jsonb_array_elements(s.policy_snapshot->'mappings') v WHERE v->>'role'='receivable';
 SELECT (v->>'accountId')::uuid INTO counterpart FROM jsonb_array_elements(s.policy_snapshot->'mappings') v WHERE v->>'role'=CASE WHEN s.effect IN ('collection','refund') THEN 'payment_clearing' ELSE 'revenue' END;
 IF s.tax_minor<>0 THEN
  net:=s.amount_minor-s.tax_minor;
  IF s.tax_account_id IS NULL OR s.effect IN ('collection','refund')
   OR (SELECT count(*) FROM archava_accounting.journal_lines WHERE tenant_id=NEW.tenant_id AND journal_id=j.id)<>(CASE WHEN net=0 THEN 2 ELSE 3 END)
   OR NOT EXISTS(SELECT 1 FROM archava_accounting.journal_lines WHERE tenant_id=NEW.tenant_id AND journal_id=j.id AND account_id=ar
    AND debit_minor=(CASE WHEN s.amount_minor>0 THEN abs(s.amount_minor) ELSE 0 END) AND credit_minor=(CASE WHEN s.amount_minor>0 THEN 0 ELSE abs(s.amount_minor) END))
   OR NOT EXISTS(SELECT 1 FROM archava_accounting.journal_lines WHERE tenant_id=NEW.tenant_id AND journal_id=j.id AND account_id=s.tax_account_id
    AND credit_minor=(CASE WHEN s.amount_minor>0 THEN abs(s.tax_minor) ELSE 0 END) AND debit_minor=(CASE WHEN s.amount_minor>0 THEN 0 ELSE abs(s.tax_minor) END))
   OR (net<>0 AND NOT EXISTS(SELECT 1 FROM archava_accounting.journal_lines WHERE tenant_id=NEW.tenant_id AND journal_id=j.id AND account_id=counterpart
    AND credit_minor=(CASE WHEN s.amount_minor>0 THEN abs(net) ELSE 0 END) AND debit_minor=(CASE WHEN s.amount_minor>0 THEN 0 ELSE abs(net) END))) THEN
   RAISE EXCEPTION 'A taxed source draft must use the captured amount, net and tax on the receivable, revenue and tax control accounts' USING ERRCODE='23514';END IF;
  RETURN NEW;
 END IF;
 debit:=CASE WHEN s.effect IN ('collection','refund') THEN counterpart ELSE ar END;
 credit:=CASE WHEN s.effect IN ('collection','refund') THEN ar ELSE counterpart END;
 IF s.amount_minor<0 THEN ar:=debit;debit:=credit;credit:=ar;END IF;
 IF (SELECT count(*) FROM archava_accounting.journal_lines WHERE tenant_id=NEW.tenant_id AND journal_id=j.id)<>2 OR NOT EXISTS(
  SELECT 1 FROM archava_accounting.journal_lines WHERE tenant_id=NEW.tenant_id AND journal_id=j.id AND account_id=debit AND debit_minor=abs(s.amount_minor) AND credit_minor=0) OR NOT EXISTS(
  SELECT 1 FROM archava_accounting.journal_lines WHERE tenant_id=NEW.tenant_id AND journal_id=j.id AND account_id=credit AND credit_minor=abs(s.amount_minor) AND debit_minor=0) THEN
  RAISE EXCEPTION 'Source draft must use the exact captured amount and account roles' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
