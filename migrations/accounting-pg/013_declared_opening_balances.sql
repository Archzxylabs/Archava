-- Declare a reviewed opening balance on a receivable, payable or inventory
-- control account. This migration imports no documents and no history: an
-- opening is one lump amount per declaration, bound to exactly one journal.
DO $$ BEGIN
 PERFORM pg_advisory_xact_lock(hashtext('archava-serving-writer-takeover'));
 IF to_regclass('archava_control.writer_lease') IS NOT NULL THEN
  PERFORM 1 FROM archava_control.writer_lease WHERE lease_name='archava-serving' FOR UPDATE;
 END IF;
END $$;
-- The declaration shares its journal's id, so one declaration is one journal.
CREATE TABLE archava_accounting.opening_balances(
 tenant_id text NOT NULL,id uuid NOT NULL,book_id uuid NOT NULL,
 control text NOT NULL CHECK(control IN ('receivable','payable','inventory')),
 control_account_id uuid NOT NULL,offset_account_id uuid NOT NULL,
 amount_minor bigint NOT NULL CHECK(amount_minor BETWEEN 1 AND 9000000000000000),
 created_by text NOT NULL,created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,book_id,id),
 CHECK(control_account_id<>offset_account_id),
 FOREIGN KEY(tenant_id,book_id,id) REFERENCES archava_accounting.journals(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,book_id,control_account_id) REFERENCES archava_accounting.accounts(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,book_id,offset_account_id) REFERENCES archava_accounting.accounts(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,created_by) REFERENCES archava_accounting.memberships(tenant_id,principal_id)
);
CREATE INDEX opening_control ON archava_accounting.opening_balances(tenant_id,book_id,control,control_account_id);
ALTER TABLE archava_accounting.opening_balances ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_accounting.opening_balances FORCE ROW LEVEL SECURITY;
CREATE POLICY opening_tenant ON archava_accounting.opening_balances
 USING(tenant_id=current_setting('archava.tenant_id',true)) WITH CHECK(tenant_id=current_setting('archava.tenant_id',true));

-- The control account is the one the book's current policy declares for the
-- role, so an opening cannot make an arbitrary account a control account.
CREATE FUNCTION archava_accounting.opening_control_account(wanted_tenant text,wanted_book uuid,wanted_control text)
 RETURNS uuid LANGUAGE sql STABLE SET search_path=pg_catalog AS $$
 SELECT CASE WHEN wanted_control='receivable' THEN
  (SELECT (v->>'accountId')::uuid FROM archava_accounting.invoice_policies p CROSS JOIN LATERAL jsonb_array_elements(p.mappings) v
   WHERE p.tenant_id=wanted_tenant AND p.book_id=wanted_book AND v->>'role'='receivable')
 ELSE
  (SELECT (v->>'accountId')::uuid FROM archava_accounting.purchase_policies p CROSS JOIN LATERAL jsonb_array_elements(p.mappings) v
   WHERE p.tenant_id=wanted_tenant AND p.book_id=wanted_book AND v->>'role'=wanted_control)
 END
$$;

CREATE FUNCTION archava_accounting.validate_opening(o archava_accounting.opening_balances)
 RETURNS void LANGUAGE plpgsql VOLATILE SET search_path=pg_catalog AS $$
DECLARE j archava_accounting.journals;control_debit bigint;control_credit bigint;
BEGIN
 PERFORM 1 FROM archava_accounting.books WHERE tenant_id=o.tenant_id AND id=o.book_id AND source_authority='native' FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'An existing native book is required' USING ERRCODE='23514';END IF;
 SELECT * INTO j FROM archava_accounting.journals WHERE tenant_id=o.tenant_id AND book_id=o.book_id AND id=o.id;
 IF j.id IS NULL OR j.state<>'draft' OR j.reversal_of IS NOT NULL OR j.created_by<>o.created_by
 OR j.source_ref<>'opening:'||o.control||':'||j.code THEN
  RAISE EXCEPTION 'An opening balance requires its own draft journal and reserved reference' USING ERRCODE='23514';END IF;
 IF archava_accounting.opening_control_account(o.tenant_id,o.book_id,o.control) IS DISTINCT FROM o.control_account_id THEN
  RAISE EXCEPTION 'An opening balance must use the control account the current policy declares' USING ERRCODE='23514';END IF;
 IF NOT EXISTS(SELECT 1 FROM archava_accounting.accounts WHERE tenant_id=o.tenant_id AND book_id=o.book_id AND id=o.control_account_id AND active
  AND class=CASE WHEN o.control='payable' THEN 'liability' ELSE 'asset' END)
 OR NOT EXISTS(SELECT 1 FROM archava_accounting.accounts WHERE tenant_id=o.tenant_id AND book_id=o.book_id AND id=o.offset_account_id AND active AND class='equity') THEN
  RAISE EXCEPTION 'An opening balance needs an active control account and an active equity offset account' USING ERRCODE='23514';END IF;
 -- Natural side only: receivable and inventory are debits, payable is a credit.
 control_debit:=CASE WHEN o.control='payable' THEN 0 ELSE o.amount_minor END;
 control_credit:=CASE WHEN o.control='payable' THEN o.amount_minor ELSE 0 END;
 IF (SELECT count(*) FROM archava_accounting.journal_lines WHERE tenant_id=o.tenant_id AND journal_id=o.id)<>2
 OR NOT EXISTS(SELECT 1 FROM archava_accounting.journal_lines l JOIN archava_accounting.accounts a ON a.tenant_id=l.tenant_id AND a.id=l.account_id
  WHERE l.tenant_id=o.tenant_id AND l.journal_id=o.id AND l.ordinal=1 AND l.account_id=o.control_account_id AND l.account_revision=a.revision
  AND l.debit_minor=control_debit AND l.credit_minor=control_credit)
 OR NOT EXISTS(SELECT 1 FROM archava_accounting.journal_lines l JOIN archava_accounting.accounts a ON a.tenant_id=l.tenant_id AND a.id=l.account_id
  WHERE l.tenant_id=o.tenant_id AND l.journal_id=o.id AND l.ordinal=2 AND l.account_id=o.offset_account_id AND l.account_revision=a.revision
  AND l.debit_minor=control_credit AND l.credit_minor=control_debit) THEN
  RAISE EXCEPTION 'Opening lines differ from the declared accounts or exact amount' USING ERRCODE='23514';END IF;
END $$;

CREATE FUNCTION archava_accounting.guard_opening() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN PERFORM archava_accounting.validate_opening(NEW);RETURN NEW;END $$;
CREATE TRIGGER opening_guard BEFORE INSERT ON archava_accounting.opening_balances FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_opening();
CREATE TRIGGER opening_immutable BEFORE UPDATE OR DELETE ON archava_accounting.opening_balances FOR EACH ROW EXECUTE FUNCTION archava_accounting.immutable();

-- A declaration exists exactly when its journal was created by the opening
-- command. A generic journal cannot take the reserved reference.
CREATE FUNCTION archava_accounting.assert_opening_claim() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE j archava_accounting.journals;o archava_accounting.opening_balances;op text;actor text;
BEGIN
 SELECT * INTO j FROM archava_accounting.journals WHERE tenant_id=NEW.tenant_id AND id=NEW.id;
 SELECT * INTO o FROM archava_accounting.opening_balances WHERE tenant_id=NEW.tenant_id AND id=NEW.id;
 SELECT a.operation,a.actor_id INTO op,actor FROM archava_accounting.audit a
 WHERE a.tenant_id=NEW.tenant_id AND a.resource_kind='journal' AND a.resource_id=NEW.id AND a.revision=1;
 IF o.id IS NOT NULL THEN
  IF op IS DISTINCT FROM 'opening_balance.draft' OR actor IS DISTINCT FROM o.created_by THEN
   RAISE EXCEPTION 'An opening balance must retain its reviewed command claim' USING ERRCODE='23514';END IF;
 ELSIF op='opening_balance.draft' OR j.source_ref LIKE 'opening:%' THEN
  RAISE EXCEPTION 'Reserved opening references require a declared opening balance' USING ERRCODE='23514';
 END IF;RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER opening_journal_claim_complete AFTER INSERT ON archava_accounting.journals DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_accounting.assert_opening_claim();
CREATE CONSTRAINT TRIGGER opening_claim_complete AFTER INSERT ON archava_accounting.opening_balances DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_accounting.assert_opening_claim();

-- Posting re-checks the declaration against the accounts and policy as they
-- are now. Voiding a draft and reversing a posted opening stay available.
CREATE FUNCTION archava_accounting.guard_opening_posting() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE o archava_accounting.opening_balances;
BEGIN
 IF NEW.state='posted' AND OLD.state<>'posted' THEN
  SELECT * INTO o FROM archava_accounting.opening_balances WHERE tenant_id=NEW.tenant_id AND id=NEW.id;
  IF o.id IS NOT NULL THEN PERFORM archava_accounting.validate_opening(o);END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER opening_posting_guard BEFORE UPDATE ON archava_accounting.journals FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_opening_posting();

CREATE FUNCTION archava_accounting.opening_guards_complete() RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog AS $$
 SELECT NOT EXISTS(SELECT 1 FROM (VALUES
 ('opening_balances','opening_guard','archava_accounting.guard_opening()',7,false,false),
 ('opening_balances','opening_immutable','archava_accounting.immutable()',27,false,false),
 ('opening_balances','opening_claim_complete','archava_accounting.assert_opening_claim()',5,true,true),
 ('journals','opening_journal_claim_complete','archava_accounting.assert_opening_claim()',5,true,true),
 ('journals','opening_posting_guard','archava_accounting.guard_opening_posting()',19,false,false)
 ) AS expected(relation,name,fn,kind,deferred_flag,initially_flag)
 WHERE NOT EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid=to_regclass('archava_accounting.'||expected.relation) AND t.tgname=expected.name AND t.tgfoid=to_regprocedure(expected.fn)
 AND t.tgtype=expected.kind AND t.tgdeferrable=expected.deferred_flag AND t.tginitdeferred=expected.initially_flag AND t.tgnargs=0
 AND t.tgenabled='O' AND NOT t.tgisinternal AND t.tgqual IS NULL AND t.tgattr=''::int2vector))
 AND EXISTS(SELECT 1 FROM pg_class WHERE oid='archava_accounting.opening_balances'::regclass AND relrowsecurity AND relforcerowsecurity)
$$;
REVOKE ALL ON FUNCTION archava_accounting.opening_guards_complete() FROM PUBLIC;
DO $$ BEGIN
 IF to_regprocedure('archava_control.install_runtime_write_guards()') IS NOT NULL THEN
  PERFORM archava_control.install_runtime_write_guards();
 END IF;
END $$;
