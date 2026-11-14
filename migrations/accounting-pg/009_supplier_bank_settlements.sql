-- Record reviewed supplier payments against actual manual bank evidence.
-- This migration sends no money and imports no historical settlement.
DO $$ BEGIN
 PERFORM pg_advisory_xact_lock(hashtext('archava-serving-writer-takeover'));
 IF to_regclass('archava_control.writer_lease') IS NOT NULL THEN
  PERFORM 1 FROM archava_control.writer_lease WHERE lease_name='archava-serving' FOR UPDATE;
 END IF;
END $$;
ALTER TABLE archava_accounting.memberships DROP CONSTRAINT memberships_permissions_check;
ALTER TABLE archava_accounting.memberships ADD CONSTRAINT memberships_permissions_check CHECK(permissions <@ ARRAY[
 'books.read','books.write','accounts.write','journals.write','journals.post','periods.close','periods.reopen','reports.read','audit.read',
 'bank.read','bank.configure','bank.import','bank.reconcile','payables.read','payables.write']::text[]);
-- Existing memberships deliberately receive no new permissions.
CREATE TABLE archava_accounting.payable_settlements(
 tenant_id text NOT NULL,id uuid NOT NULL,book_id uuid NOT NULL,source_id uuid NOT NULL,
 document_revision integer NOT NULL CHECK(document_revision>0),bank_row_id uuid NOT NULL,
 payable_account_id uuid NOT NULL,cash_account_id uuid NOT NULL,
 amount_minor bigint NOT NULL CHECK(amount_minor BETWEEN 1 AND 9000000000000000),
 effect text NOT NULL CHECK(effect IN ('payment','return')),original_payment_id uuid,
 created_by text NOT NULL,created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,book_id,id),
 CHECK(payable_account_id<>cash_account_id),
 CHECK((effect='payment' AND original_payment_id IS NULL) OR (effect='return' AND original_payment_id IS NOT NULL AND original_payment_id<>id)),
 FOREIGN KEY(tenant_id,book_id,id) REFERENCES archava_accounting.journals(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,book_id,source_id) REFERENCES archava_accounting.purchase_sources(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,book_id,bank_row_id) REFERENCES archava_accounting.bank_rows(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,book_id,payable_account_id) REFERENCES archava_accounting.accounts(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,book_id,cash_account_id) REFERENCES archava_accounting.accounts(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,book_id,original_payment_id) REFERENCES archava_accounting.payable_settlements(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,created_by) REFERENCES archava_accounting.memberships(tenant_id,principal_id)
);
CREATE INDEX payable_source ON archava_accounting.payable_settlements(tenant_id,book_id,source_id);
CREATE INDEX payable_bank_row ON archava_accounting.payable_settlements(tenant_id,bank_row_id);
CREATE INDEX payable_original_payment ON archava_accounting.payable_settlements(tenant_id,original_payment_id);
ALTER TABLE archava_accounting.payable_settlements ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_accounting.payable_settlements FORCE ROW LEVEL SECURITY;
CREATE POLICY payable_tenant ON archava_accounting.payable_settlements
 USING(tenant_id=current_setting('archava.tenant_id',true)) WITH CHECK(tenant_id=current_setting('archava.tenant_id',true));

-- Only a posted correction changes the recognized liability. Operational void
-- alone must not conceal an unposted correction from the financial subledger.
CREATE FUNCTION archava_accounting.payable_balance(wanted_tenant text,wanted_source uuid,excluded uuid DEFAULT NULL)
 RETURNS jsonb LANGUAGE sql VOLATILE SET search_path=pg_catalog AS $$
 WITH source AS(SELECT s.* FROM archava_accounting.purchase_sources s WHERE s.tenant_id=wanted_tenant AND s.id=wanted_source AND s.effect='bill'),
 recognized AS(SELECT s.*,
 EXISTS(SELECT 1 FROM archava_accounting.purchase_journal_sources x JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id WHERE x.tenant_id=s.tenant_id AND x.source_id=s.id AND j.state='posted') posted,
 coalesce((SELECT sum(v.goods_minor::numeric) FROM archava_accounting.purchase_sources v WHERE v.tenant_id=s.tenant_id AND v.book_id=s.book_id AND v.effect='void' AND v.source_snapshot->'priorMatchedVersion'->>'request_id'=s.id::text AND EXISTS(SELECT 1 FROM archava_accounting.purchase_journal_sources x JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id WHERE x.tenant_id=v.tenant_id AND x.source_id=v.id AND j.state='posted')),0) correction
 FROM source s),
 used AS(SELECT coalesce(sum(CASE WHEN p.effect='payment' THEN p.amount_minor::numeric ELSE -p.amount_minor::numeric END) FILTER(WHERE j.state='posted'),0) paid,
 coalesce(sum(p.amount_minor::numeric) FILTER(WHERE p.effect='payment' AND j.state='draft'),0) reserved
 FROM archava_accounting.payable_settlements p JOIN archava_accounting.journals j ON j.tenant_id=p.tenant_id AND j.id=p.id
 WHERE p.tenant_id=wanted_tenant AND p.source_id=wanted_source AND (excluded IS NULL OR p.id<>excluded))
 SELECT jsonb_build_object('originalMinor',s.goods_minor::text,'recognizedMinor',(CASE WHEN posted THEN s.goods_minor::numeric+correction ELSE 0 END)::text,
 'postedPaymentMinor',u.paid::text,'reservedPaymentMinor',u.reserved::text,
 'outstandingMinor',(CASE WHEN posted THEN s.goods_minor::numeric+correction ELSE 0 END-u.paid)::text,
 'availableMinor',(CASE WHEN posted THEN s.goods_minor::numeric+correction ELSE 0 END-u.paid-u.reserved)::text,'billPosted',posted)
 FROM recognized s CROSS JOIN used u
$$;

-- Reserve only the unmatched portion. A posted bank allocation consumes the
-- same evidence and must not count it a second time. Unmatching restores this
-- reservation; it does not undo the payment or its posted financial effect.
CREATE FUNCTION archava_accounting.payable_row_reserved(wanted_tenant text,wanted_row uuid,excluded uuid DEFAULT NULL)
 RETURNS numeric LANGUAGE sql VOLATILE SET search_path=pg_catalog AS $$
 SELECT coalesce(sum(s.amount_minor::numeric-coalesce((SELECT sum(a.amount_minor::numeric)
 FROM archava_accounting.bank_allocations a WHERE a.tenant_id=s.tenant_id AND a.journal_id=s.id AND a.bank_row_id=s.bank_row_id AND a.journal_line_ordinal=2
 AND NOT EXISTS(SELECT 1 FROM archava_accounting.bank_unmatches x WHERE x.tenant_id=a.tenant_id AND x.allocation_id=a.id)),0)),0)
 FROM archava_accounting.payable_settlements s JOIN archava_accounting.journals j ON j.tenant_id=s.tenant_id AND j.id=s.id
 WHERE s.tenant_id=wanted_tenant AND s.bank_row_id=wanted_row AND j.state IN ('draft','posted') AND (excluded IS NULL OR s.id<>excluded)
$$;

CREATE FUNCTION archava_accounting.validate_payable(s archava_accounting.payable_settlements)
 RETURNS void LANGUAGE plpgsql VOLATILE SET search_path=pg_catalog AS $$
DECLARE bill archava_accounting.purchase_sources;doc archava_purchasing.supplier_documents;
 j archava_accounting.journals;r archava_accounting.bank_rows;binding archava_accounting.bank_accounts;
 original archava_accounting.payable_settlements;original_j archava_accounting.journals;
 original_payable uuid;held numeric;remaining numeric;balance jsonb;
BEGIN
 PERFORM 1 FROM archava_accounting.books WHERE tenant_id=s.tenant_id AND id=s.book_id AND source_authority='native' FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'An existing native book is required' USING ERRCODE='23514';END IF;
 SELECT * INTO bill FROM archava_accounting.purchase_sources WHERE tenant_id=s.tenant_id AND book_id=s.book_id AND id=s.source_id AND effect='bill';
 SELECT * INTO j FROM archava_accounting.journals WHERE tenant_id=s.tenant_id AND book_id=s.book_id AND id=s.id;
 SELECT * INTO r FROM archava_accounting.bank_rows WHERE tenant_id=s.tenant_id AND book_id=s.book_id AND id=s.bank_row_id;
 SELECT * INTO binding FROM archava_accounting.bank_accounts WHERE tenant_id=s.tenant_id AND book_id=s.book_id AND id=r.bank_account_id;
 SELECT (m->>'accountId')::uuid INTO original_payable FROM archava_accounting.purchase_origins o CROSS JOIN LATERAL jsonb_array_elements(o.policy_snapshot->'mappings') m
 WHERE o.tenant_id=s.tenant_id AND o.id=bill.origin_id AND m->>'role'='payable';
 IF bill.id IS NULL OR bill.goods_minor<=0 OR bill.document_id IS NULL OR bill.source_revision<>s.document_revision
 OR original_payable IS DISTINCT FROM s.payable_account_id OR binding.account_id IS DISTINCT FROM s.cash_account_id
 OR r.id IS NULL OR j.id IS NULL OR j.state<>'draft' OR j.reversal_of IS NOT NULL OR j.created_by<>s.created_by OR j.entry_date<>r.entry_date OR r.entry_date<bill.source_date
 OR NOT EXISTS(SELECT 1 FROM archava_accounting.purchase_journal_sources x JOIN archava_accounting.journals b ON b.tenant_id=x.tenant_id AND b.id=x.journal_id WHERE x.tenant_id=s.tenant_id AND x.source_id=bill.id AND b.state='posted') THEN
  RAISE EXCEPTION 'Settlement requires a posted original supplier bill, its payable account and actual same-book bank evidence' USING ERRCODE='23514';END IF;
 IF NOT EXISTS(SELECT 1 FROM archava_accounting.accounts WHERE tenant_id=s.tenant_id AND book_id=s.book_id AND id=s.payable_account_id AND active AND class='liability')
 OR NOT EXISTS(SELECT 1 FROM archava_accounting.accounts WHERE tenant_id=s.tenant_id AND book_id=s.book_id AND id=s.cash_account_id AND active AND class='asset' AND cash_role='cash')
 OR (SELECT count(*) FROM archava_accounting.journal_lines WHERE tenant_id=s.tenant_id AND journal_id=s.id)<>2
 OR NOT EXISTS(SELECT 1 FROM archava_accounting.journal_lines l JOIN archava_accounting.accounts a ON a.tenant_id=l.tenant_id AND a.id=l.account_id WHERE l.tenant_id=s.tenant_id AND l.journal_id=s.id AND l.ordinal=1 AND l.account_id=s.payable_account_id AND l.account_revision=a.revision
 AND l.debit_minor=CASE WHEN s.effect='payment' THEN s.amount_minor ELSE 0 END AND l.credit_minor=CASE WHEN s.effect='return' THEN s.amount_minor ELSE 0 END)
 OR NOT EXISTS(SELECT 1 FROM archava_accounting.journal_lines l JOIN archava_accounting.accounts a ON a.tenant_id=l.tenant_id AND a.id=l.account_id WHERE l.tenant_id=s.tenant_id AND l.journal_id=s.id AND l.ordinal=2 AND l.account_id=s.cash_account_id AND l.account_revision=a.revision
 AND l.debit_minor=CASE WHEN s.effect='return' THEN s.amount_minor ELSE 0 END AND l.credit_minor=CASE WHEN s.effect='payment' THEN s.amount_minor ELSE 0 END) THEN
  RAISE EXCEPTION 'Settlement lines differ from the reviewed original accounts or exact amount' USING ERRCODE='23514';END IF;
 IF s.effect='payment' THEN
  -- Do not lock the Purchasing document after the book: Purchasing takes its
  -- document lock first. Source-void capture below shares this book fence.
  SELECT * INTO doc FROM archava_purchasing.supplier_documents WHERE tenant_id=s.tenant_id AND id=bill.document_id;
  balance:=archava_accounting.payable_balance(s.tenant_id,s.source_id,s.id);
  IF doc.state IS DISTINCT FROM 'matched' OR doc.revision<>s.document_revision OR r.amount_minor>=0 OR s.original_payment_id IS NOT NULL
  OR j.source_ref<>'payable:payment:'||s.source_id::text||':'||j.code OR (balance->>'availableMinor')::numeric<s.amount_minor THEN
   RAISE EXCEPTION 'Payment exceeds the current posted bill or live reservations' USING ERRCODE='23514';END IF;
 ELSE
  SELECT * INTO original FROM archava_accounting.payable_settlements WHERE tenant_id=s.tenant_id AND book_id=s.book_id AND id=s.original_payment_id AND effect='payment';
  SELECT * INTO original_j FROM archava_accounting.journals WHERE tenant_id=s.tenant_id AND id=original.id;
  SELECT coalesce(sum(x.amount_minor::numeric),0) INTO held FROM archava_accounting.payable_settlements x JOIN archava_accounting.journals y ON y.tenant_id=x.tenant_id AND y.id=x.id
  WHERE x.tenant_id=s.tenant_id AND x.original_payment_id=original.id AND x.id<>s.id AND y.state IN ('draft','posted');
  IF original.id IS NULL OR original_j.state IS DISTINCT FROM 'posted' OR original.source_id<>s.source_id OR original.document_revision<>s.document_revision
  OR original.payable_account_id<>s.payable_account_id OR original.cash_account_id<>s.cash_account_id OR r.amount_minor<=0 OR r.entry_date<original_j.entry_date
  OR j.source_ref<>'payable:return:'||s.original_payment_id::text||':'||j.code OR held+s.amount_minor>original.amount_minor THEN
   RAISE EXCEPTION 'Return exceeds the original posted payment or lacks actual positive bank evidence' USING ERRCODE='23514';END IF;
 END IF;
 remaining:=abs(archava_accounting.bank_row_remaining(s.tenant_id,s.bank_row_id))-archava_accounting.payable_row_reserved(s.tenant_id,s.bank_row_id,s.id);
 IF remaining IS NULL OR remaining<s.amount_minor THEN RAISE EXCEPTION 'Bank evidence is already matched or reserved' USING ERRCODE='23514';END IF;
END $$;

CREATE FUNCTION archava_accounting.guard_payable() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN PERFORM archava_accounting.validate_payable(NEW);RETURN NEW;END $$;
CREATE TRIGGER payable_guard BEFORE INSERT ON archava_accounting.payable_settlements FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_payable();
CREATE TRIGGER payable_immutable BEFORE UPDATE OR DELETE ON archava_accounting.payable_settlements FOR EACH ROW EXECUTE FUNCTION archava_accounting.immutable();

CREATE FUNCTION archava_accounting.assert_payable_claim() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE wanted uuid;j archava_accounting.journals;s archava_accounting.payable_settlements;op text;r jsonb;actor text;
BEGIN
 wanted:=NEW.id;
 SELECT * INTO j FROM archava_accounting.journals WHERE tenant_id=NEW.tenant_id AND id=wanted;
 SELECT * INTO s FROM archava_accounting.payable_settlements WHERE tenant_id=NEW.tenant_id AND id=wanted;
 SELECT a.operation,c.receipt,a.actor_id INTO op,r,actor FROM archava_accounting.audit a JOIN archava_accounting.commands c ON c.tenant_id=a.tenant_id AND c.request_id=a.request_id
 WHERE a.tenant_id=NEW.tenant_id AND a.resource_kind='journal' AND a.resource_id=wanted AND a.revision=1;
 IF s.id IS NOT NULL THEN
  IF op IS DISTINCT FROM (CASE WHEN s.effect='payment' THEN 'payable_payment.draft' ELSE 'payable_return.draft' END)
  OR actor IS DISTINCT FROM s.created_by OR r->>'sourceId' IS DISTINCT FROM s.source_id::text OR r->>'bankRowId' IS DISTINCT FROM s.bank_row_id::text
  OR r->>'originalPayablePaymentId' IS DISTINCT FROM s.original_payment_id::text THEN
   RAISE EXCEPTION 'Supplier settlement must retain its reviewed source, bank evidence and command claim' USING ERRCODE='23514';END IF;
 ELSIF op IN ('payable_payment.draft','payable_return.draft') OR j.source_ref LIKE 'payable:%' THEN
  RAISE EXCEPTION 'Reserved payable references require a structured settlement' USING ERRCODE='23514';
 END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER payable_journal_claim_complete AFTER INSERT ON archava_accounting.journals DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_accounting.assert_payable_claim();
CREATE CONSTRAINT TRIGGER payable_claim_complete AFTER INSERT ON archava_accounting.payable_settlements DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_accounting.assert_payable_claim();

CREATE FUNCTION archava_accounting.guard_payable_posting() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE s archava_accounting.payable_settlements;
BEGIN
 IF TG_OP='INSERT' THEN
  IF NEW.reversal_of IS NOT NULL AND EXISTS(SELECT 1 FROM archava_accounting.payable_settlements WHERE tenant_id=NEW.tenant_id AND id=NEW.reversal_of) THEN
   RAISE EXCEPTION 'Record an actual supplier bank return instead of reversing away a payment' USING ERRCODE='23514';END IF;
 ELSIF NEW.state='posted' THEN
  SELECT * INTO s FROM archava_accounting.payable_settlements WHERE tenant_id=NEW.tenant_id AND id=NEW.id;
  IF s.id IS NOT NULL THEN PERFORM archava_accounting.validate_payable(s);END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER payable_posting_guard BEFORE INSERT OR UPDATE ON archava_accounting.journals FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_payable_posting();

CREATE FUNCTION archava_accounting.guard_paid_bill_void() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE original uuid;b jsonb;
BEGIN
 IF NEW.effect='void' THEN
  PERFORM 1 FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
  original:=(NEW.source_snapshot->'priorMatchedVersion'->>'request_id')::uuid;
  b:=archava_accounting.payable_balance(NEW.tenant_id,original);
  IF b IS NULL OR (b->>'postedPaymentMinor')::numeric<>0 OR (b->>'reservedPaymentMinor')::numeric<>0 THEN
   RAISE EXCEPTION 'Resolve supplier payments and draft reservations before voiding the original bill' USING ERRCODE='23514';END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER payable_bill_void_guard BEFORE INSERT ON archava_accounting.purchase_sources FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_paid_bill_void();

CREATE FUNCTION archava_accounting.guard_payable_bank_match() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE s archava_accounting.payable_settlements;available numeric;
BEGIN
 PERFORM 1 FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 SELECT * INTO s FROM archava_accounting.payable_settlements WHERE tenant_id=NEW.tenant_id AND id=NEW.journal_id;
 IF s.id IS NOT NULL AND (s.bank_row_id<>NEW.bank_row_id OR NEW.journal_line_ordinal<>2) THEN
  RAISE EXCEPTION 'Supplier payment cash must match its reviewed bank evidence' USING ERRCODE='23514';END IF;
 available:=abs(archava_accounting.bank_row_remaining(NEW.tenant_id,NEW.bank_row_id))-archava_accounting.payable_row_reserved(NEW.tenant_id,NEW.bank_row_id,s.id);
 IF available IS NULL OR available<NEW.amount_minor THEN RAISE EXCEPTION 'Bank evidence is reserved for another reviewed supplier settlement' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER payable_bank_match_guard BEFORE INSERT ON archava_accounting.bank_allocations FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_payable_bank_match();

CREATE FUNCTION archava_accounting.payable_guards_complete() RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog AS $$
 SELECT NOT EXISTS(SELECT 1 FROM (VALUES
 ('payable_settlements','payable_guard','archava_accounting.guard_payable()',7,false,false),
 ('payable_settlements','payable_immutable','archava_accounting.immutable()',27,false,false),
 ('payable_settlements','payable_claim_complete','archava_accounting.assert_payable_claim()',5,true,true),
 ('journals','payable_journal_claim_complete','archava_accounting.assert_payable_claim()',5,true,true),
 ('journals','payable_posting_guard','archava_accounting.guard_payable_posting()',23,false,false),
 ('purchase_sources','payable_bill_void_guard','archava_accounting.guard_paid_bill_void()',7,false,false),
 ('bank_allocations','payable_bank_match_guard','archava_accounting.guard_payable_bank_match()',7,false,false)
 ) AS expected(relation,name,fn,kind,deferred_flag,initially_flag)
 WHERE NOT EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid=to_regclass('archava_accounting.'||expected.relation) AND t.tgname=expected.name AND t.tgfoid=to_regprocedure(expected.fn)
 AND t.tgtype=expected.kind AND t.tgdeferrable=expected.deferred_flag AND t.tginitdeferred=expected.initially_flag AND t.tgnargs=0
 AND t.tgenabled='O' AND NOT t.tgisinternal AND t.tgqual IS NULL AND t.tgattr=''::int2vector))
 AND EXISTS(SELECT 1 FROM pg_class WHERE oid='archava_accounting.payable_settlements'::regclass AND relrowsecurity AND relforcerowsecurity)
$$;
REVOKE ALL ON FUNCTION archava_accounting.payable_guards_complete() FROM PUBLIC;
DO $$ BEGIN
 IF to_regprocedure('archava_control.install_runtime_write_guards()') IS NOT NULL THEN
  PERFORM archava_control.install_runtime_write_guards();
 END IF;
END $$;
