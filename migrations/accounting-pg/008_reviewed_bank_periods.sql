-- Explicit reconciliation basis and source-bound human bank-period attestations.
DO $$ BEGIN
 PERFORM pg_advisory_xact_lock(hashtext('archava-serving-writer-takeover'));
 IF to_regclass('archava_control.writer_lease') IS NOT NULL THEN
  PERFORM 1 FROM archava_control.writer_lease WHERE lease_name='archava-serving' FOR UPDATE;
 END IF;
END $$;
ALTER TABLE archava_accounting.audit DROP CONSTRAINT audit_resource_kind_check;
ALTER TABLE archava_accounting.audit ADD CONSTRAINT audit_resource_kind_check CHECK(resource_kind IN
 ('book','account','period','journal','invoice_policy','purchase_policy','stock_policy','bank_account','bank_statement','bank_allocation','bank_unmatch','bank_policy','bank_reconciliation'));
CREATE TABLE archava_accounting.bank_policies(
 tenant_id text NOT NULL,id uuid NOT NULL,book_id uuid NOT NULL,bank_account_id uuid NOT NULL,
 from_date date NOT NULL CHECK(from_date BETWEEN '2000-01-01' AND '2100-12-31'),independent_review boolean NOT NULL,
 created_by text NOT NULL,revision integer NOT NULL DEFAULT 1 CHECK(revision>0),created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,book_id,id),UNIQUE(tenant_id,bank_account_id),
 FOREIGN KEY(tenant_id,book_id,bank_account_id) REFERENCES archava_accounting.bank_accounts(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,created_by) REFERENCES archava_accounting.memberships(tenant_id,principal_id)
);
CREATE TABLE archava_accounting.bank_reconciliations(
 tenant_id text NOT NULL,id uuid NOT NULL,book_id uuid NOT NULL,bank_policy_id uuid NOT NULL,period_id uuid NOT NULL,
 policy_revision integer NOT NULL CHECK(policy_revision>0),reviewed_period_revision integer NOT NULL CHECK(reviewed_period_revision>0),
 check_snapshot jsonb NOT NULL,created_by text NOT NULL,revision integer NOT NULL DEFAULT 1 CHECK(revision=1),created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,book_id,bank_policy_id) REFERENCES archava_accounting.bank_policies(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,book_id,period_id) REFERENCES archava_accounting.periods(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,created_by) REFERENCES archava_accounting.memberships(tenant_id,principal_id)
);
CREATE INDEX bank_reconciliation_period ON archava_accounting.bank_reconciliations(tenant_id,bank_policy_id,period_id,reviewed_period_revision);
CREATE FUNCTION archava_accounting.guard_bank_policy() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Bank reconciliation policy history cannot be deleted' USING ERRCODE='23514';END IF;
 PERFORM 1 FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id AND source_authority='native' FOR UPDATE;
 IF NOT FOUND OR NOT EXISTS(SELECT 1 FROM archava_accounting.periods WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND start_date=NEW.from_date AND state='open') THEN
  RAISE EXCEPTION 'Review an explicit opening basis at the start of an open native financial period' USING ERRCODE='23514';END IF;
 IF EXISTS(SELECT 1 FROM archava_accounting.periods WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND state='closed' AND end_date>=NEW.from_date) THEN
  RAISE EXCEPTION 'Reopen affected later periods before introducing this reconciliation basis' USING ERRCODE='23514';END IF;
 IF EXISTS(SELECT 1 FROM archava_accounting.bank_statements WHERE tenant_id=NEW.tenant_id AND bank_account_id=NEW.bank_account_id AND from_date<NEW.from_date) THEN
  RAISE EXCEPTION 'A bank policy cannot exclude already imported earlier statement evidence' USING ERRCODE='23514';END IF;
 IF TG_OP='INSERT' THEN
  IF NEW.revision<>1 THEN RAISE EXCEPTION 'Bank policy starts at revision one' USING ERRCODE='23514';END IF;
 ELSE
  IF (to_jsonb(NEW)-ARRAY['from_date','independent_review','revision']) IS DISTINCT FROM (to_jsonb(OLD)-ARRAY['from_date','independent_review','revision']) OR NEW.revision<>OLD.revision+1 OR (NEW.from_date,NEW.independent_review) IS NOT DISTINCT FROM (OLD.from_date,OLD.independent_review) OR
   EXISTS(SELECT 1 FROM archava_accounting.bank_reconciliations WHERE tenant_id=NEW.tenant_id AND bank_policy_id=NEW.id) THEN
   RAISE EXCEPTION 'Opening basis may be corrected before its first reviewed reconciliation; reviewed policy history is sealed' USING ERRCODE='23514';END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER bank_policy_guard BEFORE INSERT OR UPDATE OR DELETE ON archava_accounting.bank_policies FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_bank_policy();
CREATE CONSTRAINT TRIGGER bank_policy_complete AFTER INSERT OR UPDATE ON archava_accounting.bank_policies DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_accounting.assert_resource('bank_policy');

CREATE FUNCTION archava_accounting.bank_period_check(wanted_tenant text,wanted_policy uuid,wanted_period uuid) RETURNS jsonb LANGUAGE plpgsql VOLATILE SET search_path=pg_catalog AS $$
DECLARE p archava_accounting.bank_policies;r archava_accounting.periods;b archava_accounting.bank_accounts;
 currency_code text;covered_days bigint;statement_count bigint;first_date date;last_date date;first_opening bigint;last_closing bigint;
 opening_gl numeric;closing_gl numeric;row_count bigint;row_unmatched bigint;row_remaining numeric;leg_count bigint;leg_unmatched bigint;leg_remaining numeric;reversed_legs bigint;
 coverage_ok boolean;opening_ok boolean;closing_ok boolean;ready boolean;evidence jsonb;digest text;
BEGIN
 SELECT * INTO p FROM archava_accounting.bank_policies WHERE tenant_id=wanted_tenant AND id=wanted_policy;
 SELECT * INTO r FROM archava_accounting.periods WHERE tenant_id=wanted_tenant AND id=wanted_period AND book_id=p.book_id;
 SELECT * INTO b FROM archava_accounting.bank_accounts WHERE tenant_id=wanted_tenant AND id=p.bank_account_id AND book_id=p.book_id;
 IF p.id IS NULL OR r.id IS NULL OR b.id IS NULL THEN RETURN jsonb_build_object('ready',false,'effective',false,'reason','unknown_policy_or_period');END IF;
 IF r.end_date<p.from_date THEN RETURN jsonb_build_object('schemaVersion',1,'bankPolicyId',p.id,'policyRevision',p.revision,'periodId',r.id,'from',r.start_date,'to',r.end_date,'effectiveFrom',p.from_date,'ready',false,'effective',false,'reason','before_bank_policy_start');END IF;
 SELECT currency INTO currency_code FROM archava_accounting.books WHERE tenant_id=wanted_tenant AND id=p.book_id AND source_authority='native';
 SELECT count(*),coalesce(sum(to_date-from_date+1),0),min(from_date),max(to_date) INTO statement_count,covered_days,first_date,last_date
 FROM archava_accounting.bank_statements WHERE tenant_id=wanted_tenant AND bank_account_id=b.id AND from_date>=r.start_date AND to_date<=r.end_date;
 SELECT opening_minor INTO first_opening FROM archava_accounting.bank_statements WHERE tenant_id=wanted_tenant AND bank_account_id=b.id AND from_date>=r.start_date AND to_date<=r.end_date ORDER BY from_date LIMIT 1;
 SELECT closing_minor INTO last_closing FROM archava_accounting.bank_statements WHERE tenant_id=wanted_tenant AND bank_account_id=b.id AND from_date>=r.start_date AND to_date<=r.end_date ORDER BY to_date DESC LIMIT 1;
 SELECT coalesce(sum((l.debit_minor-l.credit_minor)::numeric) FILTER(WHERE j.entry_date<r.start_date),0),coalesce(sum((l.debit_minor-l.credit_minor)::numeric),0)
 INTO opening_gl,closing_gl FROM archava_accounting.journal_lines l JOIN archava_accounting.journals j ON j.tenant_id=l.tenant_id AND j.id=l.journal_id AND j.state='posted'
 WHERE l.tenant_id=wanted_tenant AND l.book_id=p.book_id AND l.account_id=b.account_id AND j.entry_date<=r.end_date;
 SELECT count(*),count(*) FILTER(WHERE remaining<>0),coalesce(sum(abs(remaining)),0) INTO row_count,row_unmatched,row_remaining FROM
 (SELECT archava_accounting.bank_row_remaining(x.tenant_id,x.id) AS remaining FROM archava_accounting.bank_rows x WHERE x.tenant_id=wanted_tenant AND x.bank_account_id=b.id AND x.entry_date BETWEEN r.start_date AND r.end_date) q;
 SELECT count(*),count(*) FILTER(WHERE remaining<>0 AND NOT reversal_pair),coalesce(sum(abs(remaining)) FILTER(WHERE NOT reversal_pair),0),count(*) FILTER(WHERE reversal_pair) INTO leg_count,leg_unmatched,leg_remaining,reversed_legs FROM
 (SELECT archava_accounting.bank_cash_leg_remaining(l.tenant_id,l.journal_id,l.ordinal) AS remaining,
 EXISTS(SELECT 1 FROM archava_accounting.journals other JOIN archava_accounting.journal_lines o ON o.tenant_id=other.tenant_id AND o.journal_id=other.id AND o.ordinal=l.ordinal AND o.account_id=l.account_id
  WHERE other.tenant_id=l.tenant_id AND other.book_id=l.book_id AND other.state='posted' AND other.entry_date BETWEEN r.start_date AND r.end_date
  AND (j.reversal_of=other.id OR other.reversal_of=j.id) AND o.debit_minor=l.credit_minor AND o.credit_minor=l.debit_minor
  AND archava_accounting.bank_cash_leg_remaining(l.tenant_id,l.journal_id,l.ordinal)=(l.debit_minor-l.credit_minor)::numeric
  AND archava_accounting.bank_cash_leg_remaining(o.tenant_id,o.journal_id,o.ordinal)=(o.debit_minor-o.credit_minor)::numeric) AS reversal_pair
 FROM archava_accounting.journal_lines l JOIN archava_accounting.journals j ON j.tenant_id=l.tenant_id AND j.id=l.journal_id AND j.state='posted'
 WHERE l.tenant_id=wanted_tenant AND l.book_id=p.book_id AND l.account_id=b.account_id AND j.entry_date BETWEEN r.start_date AND r.end_date) q;
 coverage_ok:=coalesce(covered_days=r.end_date-r.start_date+1 AND first_date=r.start_date AND last_date=r.end_date,false) AND statement_count>0;
 opening_ok:=coalesce(first_opening::numeric=opening_gl,false);closing_ok:=coalesce(last_closing::numeric=closing_gl,false);
 evidence:=jsonb_build_object(
 'statements',coalesce((SELECT jsonb_agg(jsonb_build_array(s.id,s.from_date,s.to_date,s.opening_minor::text,s.closing_minor::text,s.row_count) ORDER BY s.from_date,s.id) FROM archava_accounting.bank_statements s WHERE s.tenant_id=wanted_tenant AND s.bank_account_id=b.id AND s.from_date>=r.start_date AND s.to_date<=r.end_date),'[]'::jsonb),
 'rows',coalesce((SELECT jsonb_agg(jsonb_build_array(x.id,x.statement_id,x.ordinal,x.entry_date,x.amount_minor::text) ORDER BY x.id) FROM archava_accounting.bank_rows x WHERE x.tenant_id=wanted_tenant AND x.bank_account_id=b.id AND x.entry_date BETWEEN r.start_date AND r.end_date),'[]'::jsonb),
 'cashLegs',coalesce((SELECT jsonb_agg(jsonb_build_array(j.id,j.revision,j.entry_date,l.ordinal,l.debit_minor::text,l.credit_minor::text) ORDER BY j.id,l.ordinal) FROM archava_accounting.journal_lines l JOIN archava_accounting.journals j ON j.tenant_id=l.tenant_id AND j.id=l.journal_id AND j.state='posted' WHERE l.tenant_id=wanted_tenant AND l.book_id=p.book_id AND l.account_id=b.account_id AND j.entry_date<=r.end_date),'[]'::jsonb),
 'matches',coalesce((SELECT jsonb_agg(jsonb_build_array(a.id,a.bank_row_id,a.journal_id,a.journal_line_ordinal,a.amount_minor::text,u.id) ORDER BY a.id) FROM archava_accounting.bank_allocations a JOIN archava_accounting.bank_rows x ON x.tenant_id=a.tenant_id AND x.id=a.bank_row_id JOIN archava_accounting.journals j ON j.tenant_id=a.tenant_id AND j.id=a.journal_id LEFT JOIN archava_accounting.bank_unmatches u ON u.tenant_id=a.tenant_id AND u.allocation_id=a.id WHERE a.tenant_id=wanted_tenant AND x.bank_account_id=b.id AND (x.entry_date BETWEEN r.start_date AND r.end_date OR j.entry_date BETWEEN r.start_date AND r.end_date)),'[]'::jsonb));
 digest:=encode(sha256(convert_to(evidence::text,'UTF8')),'hex');
 ready:=currency_code IS NOT NULL AND p.from_date<=r.start_date AND coverage_ok AND opening_ok AND closing_ok AND row_unmatched=0 AND leg_unmatched=0;
 RETURN jsonb_build_object('schemaVersion',1,'bookId',p.book_id,'bankAccountId',b.id,'bankPolicyId',p.id,'policyRevision',p.revision,'periodId',r.id,'from',r.start_date,'to',r.end_date,'effectiveFrom',p.from_date,'currency',currency_code,
  'effective',true,'ready',ready,'independentReview',p.independent_review,'allRowsInPeriod',true,'statementCount',statement_count,'coveredDays',covered_days,'periodDays',r.end_date-r.start_date+1,
  'openingBankMinor',first_opening::text,'openingLedgerMinor',opening_gl::text,'closingBankMinor',last_closing::text,'closingLedgerMinor',closing_gl::text,
  'statementRowCount',row_count,'unmatchedStatementRows',row_unmatched,'unmatchedStatementMinor',row_remaining::text,'cashLegCount',leg_count,'unmatchedCashLegs',leg_unmatched,'unmatchedCashMinor',leg_remaining::text,'unallocatedReversalCashLegs',reversed_legs,
  'checks',jsonb_build_object('completeStatementCoverage',coverage_ok,'openingAgrees',opening_ok,'closingAgrees',closing_ok,'allStatementRowsMatched',row_unmatched=0,'allPeriodCashLegsResolved',leg_unmatched=0),'evidenceSha256',digest);
END $$;

CREATE FUNCTION archava_accounting.guard_bank_reconciliation() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE p archava_accounting.bank_policies;r archava_accounting.periods;current_check jsonb;
BEGIN
 PERFORM 1 FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id AND source_authority='native' FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'An existing native book is required' USING ERRCODE='23514';END IF;
 SELECT * INTO p FROM archava_accounting.bank_policies WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND id=NEW.bank_policy_id;
 SELECT * INTO r FROM archava_accounting.periods WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND id=NEW.period_id;
 current_check:=archava_accounting.bank_period_check(NEW.tenant_id,p.id,r.id);
 IF p.id IS NULL OR r.id IS NULL OR r.state<>'open' OR p.revision<>NEW.policy_revision OR r.revision<>NEW.reviewed_period_revision OR current_check->>'ready' IS DISTINCT FROM 'true' OR current_check IS DISTINCT FROM NEW.check_snapshot THEN
  RAISE EXCEPTION 'Review the complete current bank-period control before confirmation' USING ERRCODE='23514';END IF;
 IF p.independent_review AND EXISTS(SELECT 1 FROM archava_accounting.bank_statements s WHERE s.tenant_id=NEW.tenant_id AND s.bank_account_id=p.bank_account_id AND s.from_date>=r.start_date AND s.to_date<=r.end_date AND s.created_by=NEW.created_by) THEN
  RAISE EXCEPTION 'The bank evidence importer cannot be its independent reviewer' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE FUNCTION archava_accounting.assert_bank_reviewer() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF NOT EXISTS(SELECT 1 FROM archava_accounting.audit a JOIN archava_accounting.commands c ON c.tenant_id=a.tenant_id AND c.request_id=a.request_id WHERE a.tenant_id=NEW.tenant_id AND a.resource_kind='bank_reconciliation' AND a.resource_id=NEW.id AND a.revision=1 AND a.actor_id=NEW.created_by AND c.actor_id=NEW.created_by AND a.operation='bank_reconciliation.confirm' AND c.receipt->>'bankCheckHash'=NEW.check_snapshot->>'evidenceSha256') THEN
  RAISE EXCEPTION 'The bank reviewer must match the immutable command and audit actor' USING ERRCODE='23514';END IF;
 RETURN NULL;
END $$;
CREATE TRIGGER bank_reconciliation_guard BEFORE INSERT ON archava_accounting.bank_reconciliations FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_bank_reconciliation();
CREATE TRIGGER bank_reconciliation_immutable BEFORE UPDATE OR DELETE ON archava_accounting.bank_reconciliations FOR EACH ROW EXECUTE FUNCTION archava_accounting.immutable();
CREATE CONSTRAINT TRIGGER bank_reconciliation_complete AFTER INSERT ON archava_accounting.bank_reconciliations DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_accounting.assert_resource('bank_reconciliation');
CREATE CONSTRAINT TRIGGER bank_reviewer_complete AFTER INSERT ON archava_accounting.bank_reconciliations DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_accounting.assert_bank_reviewer();

CREATE FUNCTION archava_accounting.bank_period_close_guard() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE p archava_accounting.bank_policies;current_check jsonb;
BEGIN
 IF NEW.state<>'closed' THEN RETURN NEW;END IF;
 PERFORM 1 FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 IF EXISTS(SELECT 1 FROM archava_accounting.bank_accounts b WHERE b.tenant_id=NEW.tenant_id AND b.book_id=NEW.book_id AND NOT EXISTS(SELECT 1 FROM archava_accounting.bank_policies x WHERE x.tenant_id=b.tenant_id AND x.bank_account_id=b.id)) THEN
  RAISE EXCEPTION 'Configure an explicit reconciliation opening basis for every registered bank before closing' USING ERRCODE='23514';END IF;
 FOR p IN SELECT * FROM archava_accounting.bank_policies WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND from_date<=NEW.end_date LOOP
  current_check:=archava_accounting.bank_period_check(NEW.tenant_id,p.id,NEW.id);
  IF current_check->>'ready' IS DISTINCT FROM 'true' OR NOT EXISTS(SELECT 1 FROM archava_accounting.bank_reconciliations x WHERE x.tenant_id=NEW.tenant_id AND x.bank_policy_id=p.id AND x.period_id=NEW.id AND x.policy_revision=p.revision AND x.reviewed_period_revision=OLD.revision AND x.check_snapshot=current_check) THEN
   RAISE EXCEPTION 'Complete bank coverage, cash controls and a fresh reviewed reconciliation are required before closing' USING ERRCODE='23514';END IF;
 END LOOP;
 RETURN NEW;
END $$;
CREATE TRIGGER bank_period_close_guard BEFORE UPDATE ON archava_accounting.periods FOR EACH ROW EXECUTE FUNCTION archava_accounting.bank_period_close_guard();
-- A late write in the same transaction cannot invalidate an already executed close guard.
CREATE FUNCTION archava_accounting.assert_closed_bank_period() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE r archava_accounting.periods;p archava_accounting.bank_policies;current_check jsonb;
BEGIN
 SELECT * INTO r FROM archava_accounting.periods WHERE tenant_id=NEW.tenant_id AND id=NEW.id;
 IF r.state<>'closed' THEN RETURN NULL;END IF;
 IF EXISTS(SELECT 1 FROM archava_accounting.bank_accounts b WHERE b.tenant_id=r.tenant_id AND b.book_id=r.book_id AND NOT EXISTS(SELECT 1 FROM archava_accounting.bank_policies x WHERE x.tenant_id=b.tenant_id AND x.bank_account_id=b.id)) THEN
  RAISE EXCEPTION 'Bank opening policy is missing at close commit' USING ERRCODE='23514';END IF;
 FOR p IN SELECT * FROM archava_accounting.bank_policies WHERE tenant_id=r.tenant_id AND book_id=r.book_id AND from_date<=r.end_date LOOP
  current_check:=archava_accounting.bank_period_check(r.tenant_id,p.id,r.id);
  IF current_check->>'ready' IS DISTINCT FROM 'true' OR NOT EXISTS(SELECT 1 FROM archava_accounting.bank_reconciliations x WHERE x.tenant_id=r.tenant_id AND x.bank_policy_id=p.id AND x.period_id=r.id AND x.policy_revision=p.revision AND x.reviewed_period_revision=r.revision-1 AND x.check_snapshot=current_check) THEN
   RAISE EXCEPTION 'The closed bank period no longer matches its approved evidence at commit' USING ERRCODE='23514';END IF;
 END LOOP;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER bank_period_proof_complete AFTER UPDATE ON archava_accounting.periods DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_accounting.assert_closed_bank_period();
CREATE FUNCTION archava_accounting.guard_open_bank_row() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 PERFORM 1 FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 IF NOT EXISTS(SELECT 1 FROM archava_accounting.periods WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND state='open' AND NEW.entry_date BETWEEN start_date AND end_date) THEN
  RAISE EXCEPTION 'Statement rows cannot be appended after their financial period closed' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER bank_row_open_period_guard BEFORE INSERT ON archava_accounting.bank_rows FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_open_bank_row();
CREATE FUNCTION archava_accounting.guard_closed_bank_cash_posting() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 PERFORM 1 FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 IF NEW.state='posted' AND OLD.state<>'posted' AND EXISTS(
  SELECT 1 FROM archava_accounting.journal_lines l JOIN archava_accounting.bank_accounts b ON b.tenant_id=l.tenant_id AND b.account_id=l.account_id
  JOIN archava_accounting.bank_policies p ON p.tenant_id=b.tenant_id AND p.bank_account_id=b.id
  JOIN archava_accounting.periods r ON r.tenant_id=b.tenant_id AND r.book_id=b.book_id AND r.state='closed' AND p.from_date<=r.end_date AND NEW.entry_date<=r.end_date
  WHERE l.tenant_id=NEW.tenant_id AND l.journal_id=NEW.id) THEN
  RAISE EXCEPTION 'Reopen affected reconciled later periods before backdating cash postings' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER bank_closed_cash_posting_guard BEFORE UPDATE ON archava_accounting.journals FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_closed_bank_cash_posting();
DO $$ DECLARE owned text;BEGIN FOREACH owned IN ARRAY ARRAY['bank_policies','bank_reconciliations'] LOOP
 EXECUTE format('ALTER TABLE archava_accounting.%I ENABLE ROW LEVEL SECURITY',owned);
 EXECUTE format('ALTER TABLE archava_accounting.%I FORCE ROW LEVEL SECURITY',owned);
 EXECUTE format('CREATE POLICY bank_tenant ON archava_accounting.%I USING(tenant_id=current_setting(''archava.tenant_id'',true)) WITH CHECK(tenant_id=current_setting(''archava.tenant_id'',true))',owned);
END LOOP;END $$;
CREATE FUNCTION archava_accounting.bank_period_guards_complete() RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog AS $$
 SELECT NOT EXISTS(SELECT 1 FROM (VALUES
 ('bank_policies','bank_policy_guard','archava_accounting.guard_bank_policy()',31,false,false,0,''),
 ('bank_policies','bank_policy_complete','archava_accounting.assert_resource()',21,true,true,1,'62616e6b5f706f6c69637900'),
 ('bank_reconciliations','bank_reconciliation_guard','archava_accounting.guard_bank_reconciliation()',7,false,false,0,''),
 ('bank_reconciliations','bank_reconciliation_immutable','archava_accounting.immutable()',27,false,false,0,''),
 ('bank_reconciliations','bank_reconciliation_complete','archava_accounting.assert_resource()',5,true,true,1,'62616e6b5f7265636f6e63696c696174696f6e00'),
 ('bank_reconciliations','bank_reviewer_complete','archava_accounting.assert_bank_reviewer()',5,true,true,0,''),
 ('periods','bank_period_close_guard','archava_accounting.bank_period_close_guard()',19,false,false,0,''),
 ('periods','bank_period_proof_complete','archava_accounting.assert_closed_bank_period()',17,true,true,0,''),
 ('bank_rows','bank_row_open_period_guard','archava_accounting.guard_open_bank_row()',7,false,false,0,''),
 ('journals','bank_closed_cash_posting_guard','archava_accounting.guard_closed_bank_cash_posting()',19,false,false,0,'')
 ) AS expected(relation,name,fn,kind,deferred_flag,initially_flag,args,argument_bytes)
 WHERE NOT EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid=to_regclass('archava_accounting.'||expected.relation) AND t.tgname=expected.name AND t.tgfoid=to_regprocedure(expected.fn)
 AND t.tgtype=expected.kind AND t.tgdeferrable=expected.deferred_flag AND t.tginitdeferred=expected.initially_flag AND t.tgnargs=expected.args AND encode(t.tgargs,'hex')=expected.argument_bytes
 AND t.tgenabled='O' AND NOT t.tgisinternal AND t.tgqual IS NULL AND t.tgattr=''::int2vector))
 AND NOT EXISTS(SELECT 1 FROM pg_class c WHERE c.oid IN('archava_accounting.bank_policies'::regclass,'archava_accounting.bank_reconciliations'::regclass) AND (NOT c.relrowsecurity OR NOT c.relforcerowsecurity))
$$;
REVOKE ALL ON FUNCTION archava_accounting.bank_period_guards_complete() FROM PUBLIC;
