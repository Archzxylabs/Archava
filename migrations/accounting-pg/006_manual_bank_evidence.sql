-- Reviewed manual bank evidence. No transfers, account discovery or automatic opening balances.
DO $$ BEGIN
 PERFORM pg_advisory_xact_lock(hashtext('archava-serving-writer-takeover'));
 IF to_regclass('archava_control.writer_lease') IS NOT NULL THEN
  PERFORM 1 FROM archava_control.writer_lease WHERE lease_name='archava-serving' FOR UPDATE;
 END IF;
END $$;
ALTER TABLE archava_accounting.memberships DROP CONSTRAINT memberships_permissions_check;
ALTER TABLE archava_accounting.memberships ADD CONSTRAINT memberships_permissions_check CHECK(permissions <@ ARRAY[
 'books.read','books.write','accounts.write','journals.write','journals.post','periods.close','periods.reopen','reports.read','audit.read',
 'bank.read','bank.configure','bank.import','bank.reconcile']::text[]);
-- Existing memberships receive no new permissions.
ALTER TABLE archava_accounting.audit DROP CONSTRAINT audit_resource_kind_check;
ALTER TABLE archava_accounting.audit ADD CONSTRAINT audit_resource_kind_check CHECK(resource_kind IN
 ('book','account','period','journal','invoice_policy','purchase_policy','stock_policy','bank_account','bank_statement'));

CREATE TABLE archava_accounting.bank_accounts(
 tenant_id text NOT NULL,id uuid NOT NULL,book_id uuid NOT NULL,account_id uuid NOT NULL,account_revision integer NOT NULL CHECK(account_revision>0),
 code text NOT NULL CHECK(code ~ '^[A-Z0-9][A-Z0-9_-]{0,39}$'),name text NOT NULL CHECK(length(name) BETWEEN 1 AND 160 AND btrim(name)=name),
 revision integer NOT NULL DEFAULT 1 CHECK(revision=1),created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,book_id,id),UNIQUE(tenant_id,book_id,code),UNIQUE(tenant_id,account_id),
 FOREIGN KEY(tenant_id,book_id,account_id) REFERENCES archava_accounting.accounts(tenant_id,book_id,id)
);
CREATE TABLE archava_accounting.bank_statements(
 tenant_id text NOT NULL,id uuid NOT NULL,book_id uuid NOT NULL,bank_account_id uuid NOT NULL,account_revision integer NOT NULL CHECK(account_revision>0),
 reference text NOT NULL CHECK(length(reference) BETWEEN 1 AND 160 AND btrim(reference)=reference AND reference !~ '[[:cntrl:]]'),
 from_date date NOT NULL,to_date date NOT NULL,CHECK(from_date<=to_date AND from_date>='2000-01-01' AND to_date<='2100-12-31'),
 currency text NOT NULL,opening_minor bigint NOT NULL CHECK(opening_minor BETWEEN -9000000000000000 AND 9000000000000000),
 closing_minor bigint NOT NULL CHECK(closing_minor BETWEEN -9000000000000000 AND 9000000000000000),row_count integer NOT NULL CHECK(row_count BETWEEN 0 AND 1000),
 created_by text NOT NULL,revision integer NOT NULL DEFAULT 1 CHECK(revision=1),created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,book_id,id),UNIQUE(tenant_id,bank_account_id,reference),
 FOREIGN KEY(tenant_id,book_id,bank_account_id) REFERENCES archava_accounting.bank_accounts(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,created_by) REFERENCES archava_accounting.memberships(tenant_id,principal_id)
);
CREATE TABLE archava_accounting.bank_rows(
 tenant_id text NOT NULL,id uuid NOT NULL,book_id uuid NOT NULL,bank_account_id uuid NOT NULL,statement_id uuid NOT NULL,
 ordinal integer NOT NULL CHECK(ordinal BETWEEN 1 AND 1000),external_ref text NOT NULL CHECK(length(external_ref) BETWEEN 1 AND 160 AND btrim(external_ref)=external_ref AND external_ref !~ '[[:cntrl:]]'),
 entry_date date NOT NULL,amount_minor bigint NOT NULL CHECK(amount_minor BETWEEN -9000000000000000 AND 9000000000000000 AND amount_minor<>0),
 description text NOT NULL CHECK(length(description)<=500 AND btrim(description)=description AND description !~ '[[:cntrl:]]'),
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,book_id,id),UNIQUE(tenant_id,bank_account_id,external_ref),UNIQUE(tenant_id,statement_id,ordinal),
 FOREIGN KEY(tenant_id,book_id,statement_id) REFERENCES archava_accounting.bank_statements(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,book_id,bank_account_id) REFERENCES archava_accounting.bank_accounts(tenant_id,book_id,id)
);
CREATE INDEX bank_statement_window ON archava_accounting.bank_statements(tenant_id,bank_account_id,from_date,to_date);
CREATE INDEX bank_row_statement ON archava_accounting.bank_rows(tenant_id,statement_id,ordinal);

CREATE FUNCTION archava_accounting.guard_bank_evidence() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE b archava_accounting.books;a archava_accounting.accounts;binding archava_accounting.bank_accounts;s archava_accounting.bank_statements;
BEGIN
 SELECT * INTO b FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 IF b.id IS NULL OR b.source_authority<>'native' THEN RAISE EXCEPTION 'Manual bank evidence requires an explicit native book' USING ERRCODE='23514';END IF;
 IF TG_TABLE_NAME='bank_accounts' THEN
  SELECT * INTO a FROM archava_accounting.accounts WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND id=NEW.account_id FOR SHARE;
 ELSE
  SELECT * INTO binding FROM archava_accounting.bank_accounts WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND id=NEW.bank_account_id;
  SELECT * INTO a FROM archava_accounting.accounts WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND id=binding.account_id FOR SHARE;
 END IF;
 IF a.id IS NULL OR NOT a.active OR a.class<>'asset' OR a.cash_role<>'cash' THEN RAISE EXCEPTION 'An active reviewed cash asset is required' USING ERRCODE='23514';END IF;
 IF TG_TABLE_NAME='bank_rows' THEN
  SELECT * INTO s FROM archava_accounting.bank_statements WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND id=NEW.statement_id;
  IF s.id IS NULL OR s.bank_account_id<>NEW.bank_account_id OR NEW.entry_date<s.from_date OR NEW.entry_date>s.to_date OR
   EXISTS(SELECT 1 FROM archava_accounting.audit WHERE tenant_id=NEW.tenant_id AND resource_kind='bank_statement' AND resource_id=s.id) THEN
   RAISE EXCEPTION 'Statement rows require their original unaudited import window' USING ERRCODE='23514';END IF;
 ELSE
  IF a.revision<>NEW.account_revision THEN RAISE EXCEPTION 'The reviewed cash account revision changed' USING ERRCODE='23514';END IF;
 END IF;
 IF TG_TABLE_NAME='bank_statements' THEN
  IF NEW.currency<>b.currency OR NOT EXISTS(SELECT 1 FROM archava_accounting.periods WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND state='open' AND start_date<=NEW.from_date AND end_date>=NEW.to_date) THEN
   RAISE EXCEPTION 'Import requires book currency and a window within one open financial period' USING ERRCODE='23514';END IF;
  IF EXISTS(SELECT 1 FROM archava_accounting.bank_statements x WHERE x.tenant_id=NEW.tenant_id AND x.bank_account_id=NEW.bank_account_id AND x.from_date<=NEW.to_date AND x.to_date>=NEW.from_date) THEN
   RAISE EXCEPTION 'Statement windows must not overlap' USING ERRCODE='23514';END IF;
  IF EXISTS(SELECT 1 FROM archava_accounting.bank_statements x WHERE x.tenant_id=NEW.tenant_id AND x.bank_account_id=NEW.bank_account_id AND
   ((x.to_date+1=NEW.from_date AND x.closing_minor<>NEW.opening_minor) OR (NEW.to_date+1=x.from_date AND NEW.closing_minor<>x.opening_minor))) THEN
   RAISE EXCEPTION 'Adjacent statement closing and opening balances must agree' USING ERRCODE='23514';END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER bank_account_guard BEFORE INSERT ON archava_accounting.bank_accounts FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_bank_evidence();
CREATE TRIGGER bank_statement_guard BEFORE INSERT ON archava_accounting.bank_statements FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_bank_evidence();
CREATE TRIGGER bank_row_guard BEFORE INSERT ON archava_accounting.bank_rows FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_bank_evidence();
CREATE TRIGGER bank_account_immutable BEFORE UPDATE OR DELETE ON archava_accounting.bank_accounts FOR EACH ROW EXECUTE FUNCTION archava_accounting.immutable();
CREATE TRIGGER bank_statement_immutable BEFORE UPDATE OR DELETE ON archava_accounting.bank_statements FOR EACH ROW EXECUTE FUNCTION archava_accounting.immutable();
CREATE TRIGGER bank_row_immutable BEFORE UPDATE OR DELETE ON archava_accounting.bank_rows FOR EACH ROW EXECUTE FUNCTION archava_accounting.immutable();

CREATE FUNCTION archava_accounting.bank_statement_snapshot(wanted_tenant text,wanted_id uuid) RETURNS jsonb LANGUAGE sql STABLE SET search_path=pg_catalog AS $$
 SELECT jsonb_build_object('record',to_jsonb(s)||jsonb_build_object('opening_minor',s.opening_minor::text,'closing_minor',s.closing_minor::text),
 'rows',coalesce((SELECT jsonb_agg(to_jsonb(r)||jsonb_build_object('amount_minor',r.amount_minor::text) ORDER BY ordinal) FROM archava_accounting.bank_rows r WHERE r.tenant_id=s.tenant_id AND r.statement_id=s.id),'[]'::jsonb))
 FROM archava_accounting.bank_statements s WHERE s.tenant_id=wanted_tenant AND s.id=wanted_id
$$;
CREATE FUNCTION archava_accounting.assert_bank_statement() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE s archava_accounting.bank_statements;row_total numeric;row_count bigint;ordinals bigint;bound boolean;snapshot jsonb;events bigint;
BEGIN
 SELECT * INTO s FROM archava_accounting.bank_statements WHERE tenant_id=NEW.tenant_id AND id=NEW.id;
 SELECT count(*),coalesce(sum(amount_minor::numeric),0),coalesce(max(ordinal),0) INTO row_count,row_total,ordinals FROM archava_accounting.bank_rows WHERE tenant_id=s.tenant_id AND statement_id=s.id;
 SELECT count(*) INTO events FROM archava_accounting.audit WHERE tenant_id=s.tenant_id AND resource_kind='bank_statement' AND resource_id=s.id;
 SELECT a.snapshot,c.actor_id=s.created_by AND a.actor_id=s.created_by AND a.revision=1 AND a.book_id=s.book_id AND a.operation='bank_statement.import'
  AND c.receipt->>'requestId'=a.request_id::text AND c.receipt->>'resourceKind'='bank_statement' AND c.receipt->>'resourceId'=s.id::text
  AND c.receipt->>'bookId'=s.book_id::text AND c.receipt->>'revision'='1' AND c.receipt->>'type'='bank_statement.import'
 INTO snapshot,bound FROM archava_accounting.audit a JOIN archava_accounting.commands c ON c.tenant_id=a.tenant_id AND c.request_id=a.request_id
 WHERE a.tenant_id=s.tenant_id AND a.resource_kind='bank_statement' AND a.resource_id=s.id;
 IF row_count<>s.row_count OR ordinals<>row_count OR s.opening_minor::numeric+row_total<>s.closing_minor::numeric OR events<>1 OR bound IS DISTINCT FROM true OR snapshot IS DISTINCT FROM archava_accounting.bank_statement_snapshot(s.tenant_id,s.id) THEN
  RAISE EXCEPTION 'Bank import control total, complete rows and immutable receipt/audit must agree' USING ERRCODE='23514';END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER bank_account_complete AFTER INSERT ON archava_accounting.bank_accounts DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_accounting.assert_resource('bank_account');
CREATE CONSTRAINT TRIGGER bank_statement_complete AFTER INSERT ON archava_accounting.bank_statements DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_accounting.assert_bank_statement();

DO $$ DECLARE owned text;BEGIN FOREACH owned IN ARRAY ARRAY['bank_accounts','bank_statements','bank_rows'] LOOP
 EXECUTE format('ALTER TABLE archava_accounting.%I ENABLE ROW LEVEL SECURITY',owned);
 EXECUTE format('ALTER TABLE archava_accounting.%I FORCE ROW LEVEL SECURITY',owned);
 EXECUTE format('CREATE POLICY bank_tenant ON archava_accounting.%I USING(tenant_id=current_setting(''archava.tenant_id'',true)) WITH CHECK(tenant_id=current_setting(''archava.tenant_id'',true))',owned);
END LOOP;END $$;
CREATE FUNCTION archava_accounting.bank_evidence_guards_complete() RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog AS $$
 SELECT NOT EXISTS(SELECT 1 FROM (VALUES
 ('bank_accounts','bank_account_guard','archava_accounting.guard_bank_evidence()',7,false,false,0,''),
 ('bank_statements','bank_statement_guard','archava_accounting.guard_bank_evidence()',7,false,false,0,''),
 ('bank_rows','bank_row_guard','archava_accounting.guard_bank_evidence()',7,false,false,0,''),
 ('bank_accounts','bank_account_immutable','archava_accounting.immutable()',27,false,false,0,''),
 ('bank_statements','bank_statement_immutable','archava_accounting.immutable()',27,false,false,0,''),
 ('bank_rows','bank_row_immutable','archava_accounting.immutable()',27,false,false,0,''),
 ('bank_accounts','bank_account_complete','archava_accounting.assert_resource()',5,true,true,1,'62616e6b5f6163636f756e7400'),
 ('bank_statements','bank_statement_complete','archava_accounting.assert_bank_statement()',5,true,true,0,'')
 ) AS expected(relation,name,fn,kind,deferred_flag,initially_flag,args,argument_bytes)
 WHERE NOT EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid=to_regclass('archava_accounting.'||expected.relation) AND t.tgname=expected.name AND t.tgfoid=to_regprocedure(expected.fn)
 AND t.tgtype=expected.kind AND t.tgdeferrable=expected.deferred_flag AND t.tginitdeferred=expected.initially_flag AND t.tgnargs=expected.args AND encode(t.tgargs,'hex')=expected.argument_bytes
 AND t.tgenabled='O' AND NOT t.tgisinternal AND t.tgqual IS NULL AND t.tgattr=''::int2vector))
 AND NOT EXISTS(SELECT 1 FROM pg_class c WHERE c.oid IN('archava_accounting.bank_accounts'::regclass,'archava_accounting.bank_statements'::regclass,'archava_accounting.bank_rows'::regclass) AND (NOT c.relrowsecurity OR NOT c.relforcerowsecurity))
$$;
REVOKE ALL ON FUNCTION archava_accounting.bank_evidence_guards_complete() FROM PUBLIC;
