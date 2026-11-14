-- Match immutable manual statement rows to actual posted cash legs. No journal creation.
DO $$ BEGIN
 PERFORM pg_advisory_xact_lock(hashtext('archava-serving-writer-takeover'));
 IF to_regclass('archava_control.writer_lease') IS NOT NULL THEN
  PERFORM 1 FROM archava_control.writer_lease WHERE lease_name='archava-serving' FOR UPDATE;
 END IF;
END $$;
ALTER TABLE archava_accounting.audit DROP CONSTRAINT audit_resource_kind_check;
ALTER TABLE archava_accounting.audit ADD CONSTRAINT audit_resource_kind_check CHECK(resource_kind IN
 ('book','account','period','journal','invoice_policy','purchase_policy','stock_policy','bank_account','bank_statement','bank_allocation','bank_unmatch'));
CREATE TABLE archava_accounting.bank_allocations(
 tenant_id text NOT NULL,id uuid NOT NULL,book_id uuid NOT NULL,bank_row_id uuid NOT NULL,journal_id uuid NOT NULL,journal_line_ordinal integer NOT NULL,
 journal_revision integer NOT NULL CHECK(journal_revision>0),account_revision integer NOT NULL CHECK(account_revision>0),
 amount_minor bigint NOT NULL CHECK(amount_minor BETWEEN 1 AND 9000000000000000),
 created_by text NOT NULL,revision integer NOT NULL DEFAULT 1 CHECK(revision=1),created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,book_id,bank_row_id) REFERENCES archava_accounting.bank_rows(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,book_id,journal_id) REFERENCES archava_accounting.journals(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,journal_id,journal_line_ordinal) REFERENCES archava_accounting.journal_lines(tenant_id,journal_id,ordinal),
 FOREIGN KEY(tenant_id,created_by) REFERENCES archava_accounting.memberships(tenant_id,principal_id)
);
CREATE TABLE archava_accounting.bank_unmatches(
 tenant_id text NOT NULL,id uuid NOT NULL,book_id uuid NOT NULL,allocation_id uuid NOT NULL,created_by text NOT NULL,
 revision integer NOT NULL DEFAULT 1 CHECK(revision=1),created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,book_id,id),UNIQUE(tenant_id,allocation_id),
 FOREIGN KEY(tenant_id,book_id,allocation_id) REFERENCES archava_accounting.bank_allocations(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,created_by) REFERENCES archava_accounting.memberships(tenant_id,principal_id)
);
CREATE INDEX bank_allocation_row ON archava_accounting.bank_allocations(tenant_id,bank_row_id);
CREATE INDEX bank_allocation_cash_leg ON archava_accounting.bank_allocations(tenant_id,journal_id,journal_line_ordinal);
-- VOLATILE deliberately includes preceding changes in the same transaction/statement.
CREATE FUNCTION archava_accounting.bank_row_remaining(wanted_tenant text,wanted_id uuid) RETURNS numeric LANGUAGE sql VOLATILE SET search_path=pg_catalog AS $$
 SELECT r.amount_minor::numeric-sign(r.amount_minor)*coalesce((SELECT sum(a.amount_minor::numeric) FROM archava_accounting.bank_allocations a WHERE a.tenant_id=r.tenant_id AND a.bank_row_id=r.id AND NOT EXISTS(SELECT 1 FROM archava_accounting.bank_unmatches x WHERE x.tenant_id=a.tenant_id AND x.allocation_id=a.id)),0)
 FROM archava_accounting.bank_rows r WHERE r.tenant_id=wanted_tenant AND r.id=wanted_id
$$;
CREATE FUNCTION archava_accounting.bank_cash_leg_remaining(wanted_tenant text,wanted_journal uuid,wanted_ordinal integer) RETURNS numeric LANGUAGE sql VOLATILE SET search_path=pg_catalog AS $$
 SELECT (l.debit_minor-l.credit_minor)::numeric-sign(l.debit_minor-l.credit_minor)*coalesce((SELECT sum(a.amount_minor::numeric) FROM archava_accounting.bank_allocations a WHERE a.tenant_id=l.tenant_id AND a.journal_id=l.journal_id AND a.journal_line_ordinal=l.ordinal AND NOT EXISTS(SELECT 1 FROM archava_accounting.bank_unmatches x WHERE x.tenant_id=a.tenant_id AND x.allocation_id=a.id)),0)
 FROM archava_accounting.journal_lines l WHERE l.tenant_id=wanted_tenant AND l.journal_id=wanted_journal AND l.ordinal=wanted_ordinal
$$;
CREATE FUNCTION archava_accounting.guard_bank_match() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE original archava_accounting.bank_allocations;r archava_accounting.bank_rows;s archava_accounting.bank_statements;
 binding archava_accounting.bank_accounts;j archava_accounting.journals;l archava_accounting.journal_lines;
 a archava_accounting.accounts;row_remaining numeric;cash_remaining numeric;
BEGIN
 PERFORM 1 FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id AND source_authority='native' FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'An existing native book is required' USING ERRCODE='23514';END IF;
 IF TG_TABLE_NAME='bank_unmatches' THEN
  SELECT * INTO original FROM archava_accounting.bank_allocations WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND id=NEW.allocation_id;
  SELECT * INTO r FROM archava_accounting.bank_rows WHERE tenant_id=NEW.tenant_id AND id=original.bank_row_id;
  SELECT * INTO j FROM archava_accounting.journals WHERE tenant_id=NEW.tenant_id AND id=original.journal_id;
 ELSE
  SELECT * INTO r FROM archava_accounting.bank_rows WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND id=NEW.bank_row_id;
  SELECT * INTO j FROM archava_accounting.journals WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND id=NEW.journal_id;
 END IF;
 IF r.id IS NULL OR j.id IS NULL OR NOT EXISTS(SELECT 1 FROM archava_accounting.periods WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND state='open' AND r.entry_date BETWEEN start_date AND end_date)
 OR EXISTS(SELECT 1 FROM archava_accounting.periods WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND state='closed' AND j.entry_date BETWEEN start_date AND end_date) THEN
  RAISE EXCEPTION 'Reopen the statement and journal periods before changing a match' USING ERRCODE='23514';END IF;
 IF TG_TABLE_NAME='bank_unmatches' THEN RETURN NEW;END IF;
 SELECT * INTO s FROM archava_accounting.bank_statements WHERE tenant_id=NEW.tenant_id AND id=r.statement_id;
 SELECT * INTO binding FROM archava_accounting.bank_accounts WHERE tenant_id=NEW.tenant_id AND id=r.bank_account_id;
 SELECT * INTO a FROM archava_accounting.accounts WHERE tenant_id=NEW.tenant_id AND id=binding.account_id FOR SHARE;
 SELECT * INTO l FROM archava_accounting.journal_lines WHERE tenant_id=NEW.tenant_id AND journal_id=j.id AND ordinal=NEW.journal_line_ordinal;
 IF j.state<>'posted' OR j.revision<>NEW.journal_revision OR j.entry_date>s.to_date OR l.account_id IS DISTINCT FROM binding.account_id OR a.id IS NULL OR NOT a.active OR a.revision<>NEW.account_revision OR a.class<>'asset' OR a.cash_role<>'cash' THEN
  RAISE EXCEPTION 'Match requires the current reviewed cash account and a posted same-book cash leg' USING ERRCODE='23514';END IF;
 row_remaining:=archava_accounting.bank_row_remaining(NEW.tenant_id,r.id);
 cash_remaining:=archava_accounting.bank_cash_leg_remaining(NEW.tenant_id,j.id,l.ordinal);
 IF row_remaining IS NULL OR cash_remaining IS NULL OR row_remaining=0 OR cash_remaining=0 OR sign(row_remaining)<>sign(cash_remaining) OR NEW.amount_minor>abs(row_remaining) OR NEW.amount_minor>abs(cash_remaining) THEN
  RAISE EXCEPTION 'A bank match cannot change direction or exceed either remaining amount' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER bank_allocation_guard BEFORE INSERT ON archava_accounting.bank_allocations FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_bank_match();
CREATE TRIGGER bank_unmatch_guard BEFORE INSERT ON archava_accounting.bank_unmatches FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_bank_match();
CREATE TRIGGER bank_allocation_immutable BEFORE UPDATE OR DELETE ON archava_accounting.bank_allocations FOR EACH ROW EXECUTE FUNCTION archava_accounting.immutable();
CREATE TRIGGER bank_unmatch_immutable BEFORE UPDATE OR DELETE ON archava_accounting.bank_unmatches FOR EACH ROW EXECUTE FUNCTION archava_accounting.immutable();
CREATE CONSTRAINT TRIGGER bank_allocation_complete AFTER INSERT ON archava_accounting.bank_allocations DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_accounting.assert_resource('bank_allocation');
CREATE CONSTRAINT TRIGGER bank_unmatch_complete AFTER INSERT ON archava_accounting.bank_unmatches DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_accounting.assert_resource('bank_unmatch');
CREATE FUNCTION archava_accounting.bank_rows_close_guard() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 PERFORM 1 FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 IF NEW.state='closed' AND EXISTS(SELECT 1 FROM archava_accounting.bank_rows r WHERE r.tenant_id=NEW.tenant_id AND r.book_id=NEW.book_id AND r.entry_date BETWEEN NEW.start_date AND NEW.end_date AND archava_accounting.bank_row_remaining(r.tenant_id,r.id)<>0) THEN
  RAISE EXCEPTION 'Resolve unmatched imported statement amounts before closing' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER bank_rows_close_guard BEFORE UPDATE ON archava_accounting.periods FOR EACH ROW EXECUTE FUNCTION archava_accounting.bank_rows_close_guard();
DO $$ DECLARE owned text;BEGIN FOREACH owned IN ARRAY ARRAY['bank_allocations','bank_unmatches'] LOOP
 EXECUTE format('ALTER TABLE archava_accounting.%I ENABLE ROW LEVEL SECURITY',owned);
 EXECUTE format('ALTER TABLE archava_accounting.%I FORCE ROW LEVEL SECURITY',owned);
 EXECUTE format('CREATE POLICY bank_tenant ON archava_accounting.%I USING(tenant_id=current_setting(''archava.tenant_id'',true)) WITH CHECK(tenant_id=current_setting(''archava.tenant_id'',true))',owned);
END LOOP;END $$;
CREATE FUNCTION archava_accounting.bank_matching_guards_complete() RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog AS $$
 SELECT NOT EXISTS(SELECT 1 FROM (VALUES
 ('bank_allocations','bank_allocation_guard','archava_accounting.guard_bank_match()',7,false,false,0,''),
 ('bank_unmatches','bank_unmatch_guard','archava_accounting.guard_bank_match()',7,false,false,0,''),
 ('bank_allocations','bank_allocation_immutable','archava_accounting.immutable()',27,false,false,0,''),
 ('bank_unmatches','bank_unmatch_immutable','archava_accounting.immutable()',27,false,false,0,''),
 ('bank_allocations','bank_allocation_complete','archava_accounting.assert_resource()',5,true,true,1,'62616e6b5f616c6c6f636174696f6e00'),
 ('bank_unmatches','bank_unmatch_complete','archava_accounting.assert_resource()',5,true,true,1,'62616e6b5f756e6d6174636800'),
 ('periods','bank_rows_close_guard','archava_accounting.bank_rows_close_guard()',19,false,false,0,'')
 ) AS expected(relation,name,fn,kind,deferred_flag,initially_flag,args,argument_bytes)
 WHERE NOT EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid=to_regclass('archava_accounting.'||expected.relation) AND t.tgname=expected.name AND t.tgfoid=to_regprocedure(expected.fn)
 AND t.tgtype=expected.kind AND t.tgdeferrable=expected.deferred_flag AND t.tginitdeferred=expected.initially_flag AND t.tgnargs=expected.args AND encode(t.tgargs,'hex')=expected.argument_bytes
 AND t.tgenabled='O' AND NOT t.tgisinternal AND t.tgqual IS NULL AND t.tgattr=''::int2vector))
 AND NOT EXISTS(SELECT 1 FROM pg_class c WHERE c.oid IN('archava_accounting.bank_allocations'::regclass,'archava_accounting.bank_unmatches'::regclass) AND (NOT c.relrowsecurity OR NOT c.relforcerowsecurity))
$$;
REVOKE ALL ON FUNCTION archava_accounting.bank_matching_guards_complete() FROM PUBLIC;
