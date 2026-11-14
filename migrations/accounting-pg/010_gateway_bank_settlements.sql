-- Explicit settlement of verified provider clearing into actual manual bank evidence.
-- This migration executes no payout and infers no fee, bank event or opening balance.
DO $$ BEGIN
 PERFORM pg_advisory_xact_lock(hashtext('archava-serving-writer-takeover'));
 IF to_regclass('archava_control.writer_lease') IS NOT NULL THEN
  PERFORM 1 FROM archava_control.writer_lease WHERE lease_name='archava-serving' FOR UPDATE;
 END IF;
END $$;
ALTER TABLE archava_accounting.memberships DROP CONSTRAINT memberships_permissions_check;
ALTER TABLE archava_accounting.memberships ADD CONSTRAINT memberships_permissions_check CHECK(permissions <@ ARRAY[
 'books.read','books.write','accounts.write','journals.write','journals.post','periods.close','periods.reopen','reports.read','audit.read',
 'bank.read','bank.configure','bank.import','bank.reconcile','payables.read','payables.write','clearing.read','clearing.write']::text[]);
CREATE TABLE archava_accounting.clearing_settlements(
 tenant_id text NOT NULL,id uuid NOT NULL,book_id uuid NOT NULL,source_id uuid NOT NULL,bank_row_id uuid NOT NULL,
 clearing_account_id uuid NOT NULL,cash_account_id uuid NOT NULL,fee_account_id uuid,
 effect text NOT NULL CHECK(effect IN ('collection','refund')),
 gross_minor bigint NOT NULL CHECK(gross_minor BETWEEN 1 AND 9000000000000000),
 fee_minor bigint NOT NULL CHECK(fee_minor BETWEEN 0 AND 9000000000000000),
 cash_minor bigint NOT NULL CHECK(cash_minor BETWEEN 1 AND 9000000000000000),
 created_by text NOT NULL,created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,book_id,id),
 CHECK(clearing_account_id<>cash_account_id AND (fee_account_id IS NULL OR (fee_account_id<>cash_account_id AND fee_account_id<>clearing_account_id))),
 CHECK((fee_minor=0 AND fee_account_id IS NULL) OR (fee_minor>0 AND fee_account_id IS NOT NULL)),
 CHECK(cash_minor::numeric=CASE WHEN effect='collection' THEN gross_minor::numeric-fee_minor ELSE gross_minor::numeric+fee_minor END),
 FOREIGN KEY(tenant_id,book_id,id) REFERENCES archava_accounting.journals(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,book_id,source_id) REFERENCES archava_accounting.invoice_sources(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,book_id,bank_row_id) REFERENCES archava_accounting.bank_rows(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,book_id,clearing_account_id) REFERENCES archava_accounting.accounts(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,book_id,cash_account_id) REFERENCES archava_accounting.accounts(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,book_id,fee_account_id) REFERENCES archava_accounting.accounts(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,created_by) REFERENCES archava_accounting.memberships(tenant_id,principal_id)
);
CREATE INDEX clearing_source ON archava_accounting.clearing_settlements(tenant_id,book_id,source_id);
CREATE INDEX clearing_bank_row ON archava_accounting.clearing_settlements(tenant_id,bank_row_id);
ALTER TABLE archava_accounting.clearing_settlements ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_accounting.clearing_settlements FORCE ROW LEVEL SECURITY;
CREATE POLICY clearing_tenant ON archava_accounting.clearing_settlements USING(tenant_id=current_setting('archava.tenant_id',true)) WITH CHECK(tenant_id=current_setting('archava.tenant_id',true));

CREATE FUNCTION archava_accounting.clearing_balance(wanted_tenant text,wanted_source uuid,excluded uuid DEFAULT NULL)
 RETURNS jsonb LANGUAGE sql VOLATILE SET search_path=pg_catalog AS $$
 WITH source AS(SELECT * FROM archava_accounting.invoice_sources WHERE tenant_id=wanted_tenant AND id=wanted_source AND ((effect='collection' AND amount_minor>0) OR (effect='refund' AND amount_minor<0))),
 used AS(SELECT coalesce(sum(s.gross_minor::numeric) FILTER(WHERE j.state='posted'),0) posted,coalesce(sum(s.gross_minor::numeric) FILTER(WHERE j.state='draft'),0) reserved
 FROM archava_accounting.clearing_settlements s JOIN archava_accounting.journals j ON j.tenant_id=s.tenant_id AND j.id=s.id
 WHERE s.tenant_id=wanted_tenant AND s.source_id=wanted_source AND (excluded IS NULL OR s.id<>excluded))
 SELECT jsonb_build_object('originalGrossMinor',abs(s.amount_minor)::text,'postedGrossMinor',u.posted::text,'reservedGrossMinor',u.reserved::text,
 'unsettledGrossMinor',(abs(s.amount_minor)::numeric-u.posted)::text,'availableGrossMinor',(abs(s.amount_minor)::numeric-u.posted-u.reserved)::text,
 'sourcePosted',EXISTS(SELECT 1 FROM archava_accounting.journal_sources x JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id WHERE x.tenant_id=s.tenant_id AND x.source_id=s.id AND j.state='posted')) FROM source s CROSS JOIN used u
$$;
CREATE FUNCTION archava_accounting.clearing_row_reserved(wanted_tenant text,wanted_row uuid,excluded uuid DEFAULT NULL)
 RETURNS numeric LANGUAGE sql VOLATILE SET search_path=pg_catalog AS $$
 SELECT coalesce(sum(s.cash_minor::numeric-coalesce((SELECT sum(a.amount_minor::numeric) FROM archava_accounting.bank_allocations a
 WHERE a.tenant_id=s.tenant_id AND a.journal_id=s.id AND a.bank_row_id=s.bank_row_id AND a.journal_line_ordinal=2
 AND NOT EXISTS(SELECT 1 FROM archava_accounting.bank_unmatches x WHERE x.tenant_id=a.tenant_id AND x.allocation_id=a.id)),0)),0)
 FROM archava_accounting.clearing_settlements s JOIN archava_accounting.journals j ON j.tenant_id=s.tenant_id AND j.id=s.id
 WHERE s.tenant_id=wanted_tenant AND s.bank_row_id=wanted_row AND j.state IN ('draft','posted') AND (excluded IS NULL OR s.id<>excluded)
$$;
CREATE FUNCTION archava_accounting.bank_unreserved(wanted_tenant text,wanted_row uuid,excluded_payable uuid DEFAULT NULL,excluded_clearing uuid DEFAULT NULL)
 RETURNS numeric LANGUAGE sql VOLATILE SET search_path=pg_catalog AS $$
 SELECT abs(archava_accounting.bank_row_remaining(wanted_tenant,wanted_row))
 -archava_accounting.payable_row_reserved(wanted_tenant,wanted_row,excluded_payable)
 -archava_accounting.clearing_row_reserved(wanted_tenant,wanted_row,excluded_clearing)
$$;
CREATE FUNCTION archava_accounting.validate_clearing(s archava_accounting.clearing_settlements)
 RETURNS void LANGUAGE plpgsql VOLATILE SET search_path=pg_catalog AS $$
DECLARE original archava_accounting.invoice_sources;j archava_accounting.journals;r archava_accounting.bank_rows;
 binding archava_accounting.bank_accounts;original_clearing uuid;balance jsonb;
BEGIN
 PERFORM 1 FROM archava_accounting.books WHERE tenant_id=s.tenant_id AND id=s.book_id AND source_authority='native' FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'An existing native book is required' USING ERRCODE='23514';END IF;
 SELECT * INTO original FROM archava_accounting.invoice_sources WHERE tenant_id=s.tenant_id AND book_id=s.book_id AND id=s.source_id;
 SELECT * INTO j FROM archava_accounting.journals WHERE tenant_id=s.tenant_id AND book_id=s.book_id AND id=s.id;
 SELECT * INTO r FROM archava_accounting.bank_rows WHERE tenant_id=s.tenant_id AND book_id=s.book_id AND id=s.bank_row_id;
 SELECT * INTO binding FROM archava_accounting.bank_accounts WHERE tenant_id=s.tenant_id AND book_id=s.book_id AND id=r.bank_account_id;
 SELECT (m->>'accountId')::uuid INTO original_clearing FROM jsonb_array_elements(original.policy_snapshot->'mappings') m WHERE m->>'role'='payment_clearing';
 balance:=archava_accounting.clearing_balance(s.tenant_id,s.source_id,s.id);
 IF original.id IS NULL OR balance IS NULL OR original.effect<>s.effect OR NOT (balance->>'sourcePosted')::boolean
 OR original_clearing IS DISTINCT FROM s.clearing_account_id OR binding.account_id IS DISTINCT FROM s.cash_account_id
 OR j.id IS NULL OR j.state<>'draft' OR j.reversal_of IS NOT NULL OR j.created_by<>s.created_by
 OR r.id IS NULL OR j.entry_date<>r.entry_date OR r.entry_date<original.source_date
 OR (s.effect='collection' AND r.amount_minor<=0) OR (s.effect='refund' AND r.amount_minor>=0)
 OR j.source_ref<>'clearing:'||s.source_id::text||':'||j.code OR (balance->>'availableGrossMinor')::numeric<s.gross_minor THEN
  RAISE EXCEPTION 'Clearing settlement requires the posted original verified event, original accounts, capacity and actual bank direction' USING ERRCODE='23514';END IF;
 IF NOT EXISTS(SELECT 1 FROM archava_accounting.accounts WHERE tenant_id=s.tenant_id AND book_id=s.book_id AND id=s.clearing_account_id AND active AND class='asset' AND cash_role='none')
 OR NOT EXISTS(SELECT 1 FROM archava_accounting.accounts WHERE tenant_id=s.tenant_id AND book_id=s.book_id AND id=s.cash_account_id AND active AND class='asset' AND cash_role='cash')
 OR (s.fee_minor>0 AND NOT EXISTS(SELECT 1 FROM archava_accounting.accounts WHERE tenant_id=s.tenant_id AND book_id=s.book_id AND id=s.fee_account_id AND active AND class='expense' AND cash_role='operating'))
 OR (SELECT count(*) FROM archava_accounting.journal_lines WHERE tenant_id=s.tenant_id AND journal_id=s.id)<>(CASE WHEN s.fee_minor=0 THEN 2 ELSE 3 END) THEN
  RAISE EXCEPTION 'Clearing, cash and fee account classifications or line counts differ from the reviewed source' USING ERRCODE='23514';END IF;
 IF NOT EXISTS(SELECT 1 FROM archava_accounting.journal_lines l JOIN archava_accounting.accounts a ON a.tenant_id=l.tenant_id AND a.id=l.account_id
 WHERE l.tenant_id=s.tenant_id AND l.journal_id=s.id AND l.ordinal=1 AND l.account_id=s.clearing_account_id AND l.account_revision=a.revision
 AND l.debit_minor=CASE WHEN s.effect='refund' THEN s.gross_minor ELSE 0 END AND l.credit_minor=CASE WHEN s.effect='collection' THEN s.gross_minor ELSE 0 END)
 OR NOT EXISTS(SELECT 1 FROM archava_accounting.journal_lines l JOIN archava_accounting.accounts a ON a.tenant_id=l.tenant_id AND a.id=l.account_id
 WHERE l.tenant_id=s.tenant_id AND l.journal_id=s.id AND l.ordinal=2 AND l.account_id=s.cash_account_id AND l.account_revision=a.revision
 AND l.debit_minor=CASE WHEN s.effect='collection' THEN s.cash_minor ELSE 0 END AND l.credit_minor=CASE WHEN s.effect='refund' THEN s.cash_minor ELSE 0 END)
 OR (s.fee_minor>0 AND NOT EXISTS(SELECT 1 FROM archava_accounting.journal_lines l JOIN archava_accounting.accounts a ON a.tenant_id=l.tenant_id AND a.id=l.account_id
 WHERE l.tenant_id=s.tenant_id AND l.journal_id=s.id AND l.ordinal=3 AND l.account_id=s.fee_account_id AND l.account_revision=a.revision AND l.debit_minor=s.fee_minor AND l.credit_minor=0)) THEN
  RAISE EXCEPTION 'Clearing settlement cannot repost AR, revenue or an inferred balancing fee' USING ERRCODE='23514';END IF;
 IF archava_accounting.bank_unreserved(s.tenant_id,s.bank_row_id,NULL,s.id)<s.cash_minor THEN
  RAISE EXCEPTION 'Bank evidence is already matched or reserved for another supplier or clearing settlement' USING ERRCODE='23514';END IF;
END $$;
CREATE FUNCTION archava_accounting.guard_clearing() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN PERFORM archava_accounting.validate_clearing(NEW);RETURN NEW;END $$;
CREATE TRIGGER clearing_guard BEFORE INSERT ON archava_accounting.clearing_settlements FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_clearing();
CREATE TRIGGER clearing_immutable BEFORE UPDATE OR DELETE ON archava_accounting.clearing_settlements FOR EACH ROW EXECUTE FUNCTION archava_accounting.immutable();
CREATE FUNCTION archava_accounting.assert_clearing_claim() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE j archava_accounting.journals;s archava_accounting.clearing_settlements;op text;r jsonb;actor text;
BEGIN
 SELECT * INTO j FROM archava_accounting.journals WHERE tenant_id=NEW.tenant_id AND id=NEW.id;
 SELECT * INTO s FROM archava_accounting.clearing_settlements WHERE tenant_id=NEW.tenant_id AND id=NEW.id;
 SELECT a.operation,c.receipt,a.actor_id INTO op,r,actor FROM archava_accounting.audit a JOIN archava_accounting.commands c ON c.tenant_id=a.tenant_id AND c.request_id=a.request_id
 WHERE a.tenant_id=NEW.tenant_id AND a.resource_kind='journal' AND a.resource_id=NEW.id AND a.revision=1;
 IF s.id IS NOT NULL THEN
  IF op IS DISTINCT FROM 'clearing_settlement.draft' OR actor IS DISTINCT FROM s.created_by OR r->>'sourceId' IS DISTINCT FROM s.source_id::text OR r->>'bankRowId' IS DISTINCT FROM s.bank_row_id::text THEN
   RAISE EXCEPTION 'Clearing settlement must retain the reviewed original event and actual bank evidence' USING ERRCODE='23514';END IF;
 ELSIF op='clearing_settlement.draft' OR j.source_ref LIKE 'clearing:%' THEN
  RAISE EXCEPTION 'Reserved clearing references require a structured settlement' USING ERRCODE='23514';
 END IF;RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER clearing_journal_claim_complete AFTER INSERT ON archava_accounting.journals DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_accounting.assert_clearing_claim();
CREATE CONSTRAINT TRIGGER clearing_claim_complete AFTER INSERT ON archava_accounting.clearing_settlements DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_accounting.assert_clearing_claim();
-- Append guards for shared AP/clearing reservations; Accounting009 stays immutable.
CREATE FUNCTION archava_accounting.guard_payable_shared_bank_reserve() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 PERFORM 1 FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 IF archava_accounting.bank_unreserved(NEW.tenant_id,NEW.bank_row_id,NEW.id,NULL)<NEW.amount_minor THEN
  RAISE EXCEPTION 'Supplier payment cannot consume bank evidence reserved by a clearing settlement' USING ERRCODE='23514';END IF;RETURN NEW;
END $$;
CREATE TRIGGER clearing_payable_shared_reserve_guard BEFORE INSERT ON archava_accounting.payable_settlements FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_payable_shared_bank_reserve();
CREATE FUNCTION archava_accounting.guard_clearing_posting() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE s archava_accounting.clearing_settlements;p archava_accounting.payable_settlements;
BEGIN
 IF TG_OP='INSERT' THEN
  IF NEW.reversal_of IS NOT NULL AND EXISTS(SELECT 1 FROM archava_accounting.clearing_settlements WHERE tenant_id=NEW.tenant_id AND id=NEW.reversal_of) THEN
   RAISE EXCEPTION 'A ledger-only reversal cannot erase an actual clearing bank movement' USING ERRCODE='23514';END IF;
 ELSIF NEW.state='posted' THEN
  SELECT * INTO s FROM archava_accounting.clearing_settlements WHERE tenant_id=NEW.tenant_id AND id=NEW.id;
  IF s.id IS NOT NULL THEN PERFORM archava_accounting.validate_clearing(s);END IF;
  SELECT * INTO p FROM archava_accounting.payable_settlements WHERE tenant_id=NEW.tenant_id AND id=NEW.id;
  IF p.id IS NOT NULL AND archava_accounting.bank_unreserved(p.tenant_id,p.bank_row_id,p.id,NULL)<p.amount_minor THEN
   RAISE EXCEPTION 'Supplier payment bank capacity is held by another clearing settlement' USING ERRCODE='23514';END IF;
 END IF;RETURN NEW;
END $$;
CREATE TRIGGER clearing_posting_guard BEFORE INSERT OR UPDATE ON archava_accounting.journals FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_clearing_posting();
CREATE FUNCTION archava_accounting.guard_clearing_bank_match() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE s archava_accounting.clearing_settlements;p archava_accounting.payable_settlements;
BEGIN
 PERFORM 1 FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 SELECT * INTO s FROM archava_accounting.clearing_settlements WHERE tenant_id=NEW.tenant_id AND id=NEW.journal_id;
 SELECT * INTO p FROM archava_accounting.payable_settlements WHERE tenant_id=NEW.tenant_id AND id=NEW.journal_id;
 IF s.id IS NOT NULL AND (s.bank_row_id<>NEW.bank_row_id OR NEW.journal_line_ordinal<>2) THEN
  RAISE EXCEPTION 'Clearing cash must match its reviewed original bank evidence' USING ERRCODE='23514';END IF;
 IF archava_accounting.bank_unreserved(NEW.tenant_id,NEW.bank_row_id,p.id,s.id)<NEW.amount_minor THEN
  RAISE EXCEPTION 'Bank evidence is reserved by another supplier or clearing settlement' USING ERRCODE='23514';END IF;RETURN NEW;
END $$;
CREATE TRIGGER clearing_bank_match_guard BEFORE INSERT ON archava_accounting.bank_allocations FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_clearing_bank_match();

CREATE FUNCTION archava_accounting.clearing_guards_complete() RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog AS $$
 SELECT NOT EXISTS(SELECT 1 FROM (VALUES
 ('clearing_settlements','clearing_guard','archava_accounting.guard_clearing()',7,false,false),
 ('clearing_settlements','clearing_immutable','archava_accounting.immutable()',27,false,false),
 ('clearing_settlements','clearing_claim_complete','archava_accounting.assert_clearing_claim()',5,true,true),
 ('journals','clearing_journal_claim_complete','archava_accounting.assert_clearing_claim()',5,true,true),
 ('journals','clearing_posting_guard','archava_accounting.guard_clearing_posting()',23,false,false),
 ('payable_settlements','clearing_payable_shared_reserve_guard','archava_accounting.guard_payable_shared_bank_reserve()',7,false,false),
 ('bank_allocations','clearing_bank_match_guard','archava_accounting.guard_clearing_bank_match()',7,false,false)
 ) AS expected(relation,name,fn,kind,deferred_flag,initially_flag)
 WHERE NOT EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid=to_regclass('archava_accounting.'||expected.relation) AND t.tgname=expected.name AND t.tgfoid=to_regprocedure(expected.fn)
 AND t.tgtype=expected.kind AND t.tgdeferrable=expected.deferred_flag AND t.tginitdeferred=expected.initially_flag AND t.tgnargs=0
 AND t.tgenabled='O' AND NOT t.tgisinternal AND t.tgqual IS NULL AND t.tgattr=''::int2vector))
 AND EXISTS(SELECT 1 FROM pg_class WHERE oid='archava_accounting.clearing_settlements'::regclass AND relrowsecurity AND relforcerowsecurity)
$$;
REVOKE ALL ON FUNCTION archava_accounting.clearing_guards_complete() FROM PUBLIC;
DO $$ BEGIN IF to_regprocedure('archava_control.install_runtime_write_guards()') IS NOT NULL THEN PERFORM archava_control.install_runtime_write_guards();END IF;END $$;
