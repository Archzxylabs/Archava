-- Explicit company books. No historical trade is posted by installing this ledger.
CREATE TABLE archava_accounting.memberships (
 tenant_id text NOT NULL,principal_id text NOT NULL,display_name text NOT NULL DEFAULT '' CHECK(length(display_name)<=254),
 kind text NOT NULL CHECK(kind IN ('human','service')),
 permissions text[] NOT NULL CHECK(permissions <@ ARRAY['books.read','books.write','accounts.write','journals.write','journals.post','periods.close','periods.reopen','reports.read','audit.read']::text[]),
 scope text NOT NULL CHECK(scope IN ('all','assigned')),active boolean NOT NULL DEFAULT true,PRIMARY KEY(tenant_id,principal_id)
);
-- Fixed ISO catalogue; existing book metadata never follows a future catalogue silently.
CREATE FUNCTION archava_accounting.currency_minor_units(code text) RETURNS smallint LANGUAGE sql IMMUTABLE SET search_path=pg_catalog AS $$
 SELECT minor_units::smallint FROM (VALUES
 ('AED',2),
 ('AFN',2),
 ('ALL',2),
 ('AMD',2),
 ('AOA',2),
 ('ARS',2),
 ('AUD',2),
 ('AWG',2),
 ('AZN',2),
 ('BAM',2),
 ('BBD',2),
 ('BDT',2),
 ('BHD',3),
 ('BIF',0),
 ('BMD',2),
 ('BND',2),
 ('BOB',2),
 ('BOV',2),
 ('BRL',2),
 ('BSD',2),
 ('BTN',2),
 ('BWP',2),
 ('BYN',2),
 ('BZD',2),
 ('CAD',2),
 ('CDF',2),
 ('CHE',2),
 ('CHF',2),
 ('CHW',2),
 ('CLF',4),
 ('CLP',0),
 ('CNY',2),
 ('COP',2),
 ('COU',2),
 ('CRC',2),
 ('CUP',2),
 ('CVE',2),
 ('CZK',2),
 ('DJF',0),
 ('DKK',2),
 ('DOP',2),
 ('DZD',2),
 ('EGP',2),
 ('ERN',2),
 ('ETB',2),
 ('EUR',2),
 ('FJD',2),
 ('FKP',2),
 ('GBP',2),
 ('GEL',2),
 ('GHS',2),
 ('GIP',2),
 ('GMD',2),
 ('GNF',0),
 ('GTQ',2),
 ('GYD',2),
 ('HKD',2),
 ('HNL',2),
 ('HTG',2),
 ('HUF',2),
 ('IDR',2),
 ('ILS',2),
 ('INR',2),
 ('IQD',3),
 ('IRR',2),
 ('ISK',0),
 ('JMD',2),
 ('JOD',3),
 ('JPY',0),
 ('KES',2),
 ('KGS',2),
 ('KHR',2),
 ('KMF',0),
 ('KPW',2),
 ('KRW',0),
 ('KWD',3),
 ('KYD',2),
 ('KZT',2),
 ('LAK',2),
 ('LBP',2),
 ('LKR',2),
 ('LRD',2),
 ('LSL',2),
 ('LYD',3),
 ('MAD',2),
 ('MDL',2),
 ('MGA',2),
 ('MKD',2),
 ('MMK',2),
 ('MNT',2),
 ('MOP',2),
 ('MRU',2),
 ('MUR',2),
 ('MVR',2),
 ('MWK',2),
 ('MXN',2),
 ('MXV',2),
 ('MYR',2),
 ('MZN',2),
 ('NAD',2),
 ('NGN',2),
 ('NIO',2),
 ('NOK',2),
 ('NPR',2),
 ('NZD',2),
 ('OMR',3),
 ('PAB',2),
 ('PEN',2),
 ('PGK',2),
 ('PHP',2),
 ('PKR',2),
 ('PLN',2),
 ('PYG',0),
 ('QAR',2),
 ('RON',2),
 ('RSD',2),
 ('RUB',2),
 ('RWF',0),
 ('SAR',2),
 ('SBD',2),
 ('SCR',2),
 ('SDG',2),
 ('SEK',2),
 ('SGD',2),
 ('SHP',2),
 ('SLE',2),
 ('SOS',2),
 ('SRD',2),
 ('SSP',2),
 ('STN',2),
 ('SVC',2),
 ('SYP',2),
 ('SZL',2),
 ('THB',2),
 ('TJS',2),
 ('TMT',2),
 ('TND',3),
 ('TOP',2),
 ('TRY',2),
 ('TTD',2),
 ('TWD',2),
 ('TZS',2),
 ('UAH',2),
 ('UGX',0),
 ('USD',2),
 ('USN',2),
 ('UYI',0),
 ('UYU',2),
 ('UYW',4),
 ('UZS',2),
 ('VED',2),
 ('VES',2),
 ('VND',0),
 ('VUV',0),
 ('WST',2),
 ('XAD',2),
 ('XAF',0),
 ('XCD',2),
 ('XCG',2),
 ('XOF',0),
 ('XPF',0),
 ('YER',2),
 ('ZAR',2),
 ('ZMW',2),
 ('ZWG',2)
 ) AS units(currency,minor_units) WHERE currency=code
$$;
CREATE TABLE archava_accounting.books (
 tenant_id text NOT NULL,id uuid NOT NULL,company_id uuid NOT NULL,company_revision integer NOT NULL CHECK(company_revision>0),
 company_code text NOT NULL,company_name text NOT NULL,owner_id text NOT NULL,
 code text NOT NULL CHECK(code ~ '^[A-Z0-9][A-Z0-9_-]{0,39}$'),name text NOT NULL CHECK(length(name) BETWEEN 1 AND 160 AND btrim(name)<>''),
 currency text NOT NULL,minor_units smallint NOT NULL CHECK(minor_units BETWEEN 0 AND 4),CHECK(archava_accounting.currency_minor_units(currency) IS NOT NULL AND minor_units=archava_accounting.currency_minor_units(currency)),currency_catalogue text NOT NULL CHECK(currency_catalogue='SIX-2026-09-17'),
 source_authority text NOT NULL CHECK(source_authority IN ('native','external')),external_system text,
 independent_posting boolean NOT NULL,revision integer NOT NULL DEFAULT 1 CHECK(revision=1),created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 CHECK((source_authority='native' AND external_system IS NULL) OR (source_authority='external' AND external_system IS NOT NULL AND external_system ~ '^[A-Z0-9][A-Z0-9_-]{0,39}$')),
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,company_id),UNIQUE(tenant_id,code),
 FOREIGN KEY(tenant_id,owner_id) REFERENCES archava_accounting.memberships(tenant_id,principal_id),
 FOREIGN KEY(tenant_id,company_id) REFERENCES archava_organizations.companies(tenant_id,id)
);
CREATE TABLE archava_accounting.accounts (
 tenant_id text NOT NULL,id uuid NOT NULL,book_id uuid NOT NULL,code text NOT NULL CHECK(code ~ '^[A-Z0-9][A-Z0-9_-]{0,39}$'),
 name text NOT NULL CHECK(length(name) BETWEEN 1 AND 160 AND btrim(name)<>''),
 class text NOT NULL CHECK(class IN ('asset','liability','equity','revenue','cost_of_goods_sold','expense')),
 cash_role text NOT NULL CHECK(cash_role IN ('none','cash','operating','investing','financing')),CHECK(cash_role<>'cash' OR class='asset'),
 active boolean NOT NULL DEFAULT true,revision integer NOT NULL DEFAULT 1 CHECK(revision>0),created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,book_id,id),UNIQUE(tenant_id,book_id,code),FOREIGN KEY(tenant_id,book_id) REFERENCES archava_accounting.books(tenant_id,id)
);
CREATE TABLE archava_accounting.periods (
 tenant_id text NOT NULL,id uuid NOT NULL,book_id uuid NOT NULL,code text NOT NULL CHECK(code ~ '^[A-Z0-9][A-Z0-9_-]{0,39}$'),
 start_date date NOT NULL,end_date date NOT NULL,CHECK(start_date<=end_date),CHECK(start_date>='2000-01-01' AND end_date<='2100-12-31'),
 state text NOT NULL DEFAULT 'open' CHECK(state IN ('open','closed')),revision integer NOT NULL DEFAULT 1 CHECK(revision>0),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,book_id,id),UNIQUE(tenant_id,book_id,code),
 FOREIGN KEY(tenant_id,book_id) REFERENCES archava_accounting.books(tenant_id,id)
);
CREATE TABLE archava_accounting.journals (
 tenant_id text NOT NULL,id uuid NOT NULL,book_id uuid NOT NULL,period_id uuid NOT NULL,
 code text NOT NULL CHECK(code ~ '^[A-Z0-9][A-Z0-9_-]{0,39}$'),entry_date date NOT NULL,
 source_ref text NOT NULL CHECK(length(source_ref) BETWEEN 1 AND 200 AND btrim(source_ref)<>''),
 memo text NOT NULL CHECK(length(memo) BETWEEN 1 AND 500 AND btrim(memo)<>''),
 created_by text NOT NULL,posted_by text,posted_at timestamptz,state text NOT NULL DEFAULT 'draft' CHECK(state IN ('draft','posted','void')),
 reversal_of uuid,revision integer NOT NULL DEFAULT 1 CHECK(revision>0),created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 CHECK((state='posted' AND posted_by IS NOT NULL AND posted_at IS NOT NULL) OR (state<>'posted' AND posted_by IS NULL AND posted_at IS NULL)),
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,book_id,id),UNIQUE(tenant_id,book_id,code),UNIQUE(tenant_id,book_id,source_ref),
 FOREIGN KEY(tenant_id,book_id) REFERENCES archava_accounting.books(tenant_id,id),
 FOREIGN KEY(tenant_id,book_id,period_id) REFERENCES archava_accounting.periods(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,book_id,reversal_of) REFERENCES archava_accounting.journals(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,created_by) REFERENCES archava_accounting.memberships(tenant_id,principal_id),
 FOREIGN KEY(tenant_id,posted_by) REFERENCES archava_accounting.memberships(tenant_id,principal_id)
);
CREATE UNIQUE INDEX one_live_reversal ON archava_accounting.journals(tenant_id,book_id,reversal_of) WHERE reversal_of IS NOT NULL AND state<>'void';
CREATE INDEX journal_period ON archava_accounting.journals(tenant_id,book_id,entry_date,id) WHERE state='posted';
CREATE TABLE archava_accounting.journal_lines (
 tenant_id text NOT NULL,journal_id uuid NOT NULL,book_id uuid NOT NULL,ordinal integer NOT NULL CHECK(ordinal BETWEEN 1 AND 100),account_id uuid NOT NULL,
 account_revision integer NOT NULL CHECK(account_revision>0),account_code text NOT NULL,account_name text NOT NULL,class text NOT NULL,cash_role text NOT NULL,
 debit_minor bigint NOT NULL CHECK(debit_minor BETWEEN 0 AND 9000000000000000),credit_minor bigint NOT NULL CHECK(credit_minor BETWEEN 0 AND 9000000000000000),
 CHECK((debit_minor>0 AND credit_minor=0) OR (credit_minor>0 AND debit_minor=0)),memo text NOT NULL CHECK(length(memo)<=500),
 PRIMARY KEY(tenant_id,journal_id,ordinal),FOREIGN KEY(tenant_id,book_id,journal_id) REFERENCES archava_accounting.journals(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,book_id,account_id) REFERENCES archava_accounting.accounts(tenant_id,book_id,id)
);
CREATE INDEX account_ledger ON archava_accounting.journal_lines(tenant_id,book_id,account_id,journal_id);
CREATE TABLE archava_accounting.commands (
 tenant_id text NOT NULL,request_id uuid NOT NULL,actor_id text NOT NULL,fingerprint text NOT NULL,receipt jsonb,
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),PRIMARY KEY(tenant_id,request_id),
 FOREIGN KEY(tenant_id,actor_id) REFERENCES archava_accounting.memberships(tenant_id,principal_id)
);
CREATE TABLE archava_accounting.audit (
 tenant_id text NOT NULL,request_id uuid NOT NULL,actor_id text NOT NULL,operation text NOT NULL,
 resource_kind text NOT NULL CHECK(resource_kind IN ('book','account','period','journal')),resource_id uuid NOT NULL,book_id uuid NOT NULL,
 revision integer NOT NULL CHECK(revision>0),reason text NOT NULL CHECK(length(reason) BETWEEN 1 AND 2000 AND btrim(reason)<>''),snapshot jsonb NOT NULL,
 occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),PRIMARY KEY(tenant_id,request_id),UNIQUE(tenant_id,resource_kind,resource_id,revision),
 FOREIGN KEY(tenant_id,request_id) REFERENCES archava_accounting.commands(tenant_id,request_id),FOREIGN KEY(tenant_id,book_id) REFERENCES archava_accounting.books(tenant_id,id)
);
CREATE FUNCTION archava_accounting.immutable() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN RAISE EXCEPTION 'Financial history is immutable' USING ERRCODE='23514';END $$;
CREATE TRIGGER book_immutable BEFORE UPDATE OR DELETE ON archava_accounting.books FOR EACH ROW EXECUTE FUNCTION archava_accounting.immutable();
CREATE TRIGGER lines_immutable BEFORE UPDATE OR DELETE ON archava_accounting.journal_lines FOR EACH ROW EXECUTE FUNCTION archava_accounting.immutable();
CREATE TRIGGER audit_immutable BEFORE UPDATE OR DELETE ON archava_accounting.audit FOR EACH ROW EXECUTE FUNCTION archava_accounting.immutable();
CREATE FUNCTION archava_accounting.guard_command() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF TG_OP='DELETE' OR OLD.receipt IS NOT NULL OR (to_jsonb(NEW)-'receipt') IS DISTINCT FROM (to_jsonb(OLD)-'receipt') OR NEW.receipt IS NULL THEN
  RAISE EXCEPTION 'Financial command is immutable' USING ERRCODE='23514';END IF;RETURN NEW;
END $$;
CREATE TRIGGER command_guard BEFORE UPDATE OR DELETE ON archava_accounting.commands FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_command();
CREATE FUNCTION archava_accounting.guard_book() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE c archava_organizations.companies%ROWTYPE;
BEGIN
 SELECT * INTO c FROM archava_organizations.companies WHERE tenant_id=NEW.tenant_id AND id=NEW.company_id FOR SHARE;
 IF NOT FOUND OR NOT c.active OR c.revision<>NEW.company_revision OR c.code<>NEW.company_code OR c.legal_name<>NEW.company_name THEN
  RAISE EXCEPTION 'Book differs from the reviewed active company' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER book_guard BEFORE INSERT ON archava_accounting.books FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_book();
CREATE FUNCTION archava_accounting.guard_account() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Account history cannot be deleted' USING ERRCODE='23514';END IF;
 PERFORM 1 FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id AND source_authority='native' FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Native book required' USING ERRCODE='23514';END IF;
 IF TG_OP='INSERT' THEN
  IF NEW.revision<>1 OR NOT NEW.active THEN RAISE EXCEPTION 'Account starts active at revision one' USING ERRCODE='23514';END IF;
 ELSE
  IF (to_jsonb(NEW)-ARRAY['active','revision']) IS DISTINCT FROM (to_jsonb(OLD)-ARRAY['active','revision']) OR NEW.revision<>OLD.revision+1 OR NEW.active=OLD.active THEN
   RAISE EXCEPTION 'Account identity and classification are immutable' USING ERRCODE='23514';END IF;
 END IF;RETURN NEW;
END $$;
CREATE TRIGGER account_guard BEFORE INSERT OR UPDATE OR DELETE ON archava_accounting.accounts FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_account();
CREATE FUNCTION archava_accounting.guard_period() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE b archava_accounting.books%ROWTYPE;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Period history cannot be deleted' USING ERRCODE='23514';END IF;
 SELECT * INTO b FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 IF NOT FOUND OR b.source_authority<>'native' THEN RAISE EXCEPTION 'Native book required' USING ERRCODE='23514';END IF;
 IF TG_OP='INSERT' THEN
  IF NEW.revision<>1 OR NEW.state<>'open' OR EXISTS(SELECT 1 FROM archava_accounting.periods p WHERE p.tenant_id=NEW.tenant_id AND p.book_id=NEW.book_id AND p.start_date<=NEW.end_date AND p.end_date>=NEW.start_date) THEN
   RAISE EXCEPTION 'Period overlaps or has invalid initial state' USING ERRCODE='23514';END IF;
 ELSE
  IF (to_jsonb(NEW)-ARRAY['state','revision']) IS DISTINCT FROM (to_jsonb(OLD)-ARRAY['state','revision']) OR NEW.revision<>OLD.revision+1 OR NEW.state=OLD.state THEN
   RAISE EXCEPTION 'Period identity is immutable' USING ERRCODE='23514';END IF;
  IF NEW.state='closed' AND EXISTS(SELECT 1 FROM archava_accounting.journals WHERE tenant_id=NEW.tenant_id AND period_id=NEW.id AND state='draft') THEN
   RAISE EXCEPTION 'Resolve draft journals before closing' USING ERRCODE='23514';END IF;
 END IF;RETURN NEW;
END $$;
CREATE TRIGGER period_guard BEFORE INSERT OR UPDATE OR DELETE ON archava_accounting.periods FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_period();
CREATE FUNCTION archava_accounting.guard_journal() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE b archava_accounting.books%ROWTYPE;p archava_accounting.periods%ROWTYPE;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Journal history cannot be deleted' USING ERRCODE='23514';END IF;
 SELECT * INTO b FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 SELECT * INTO p FROM archava_accounting.periods WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND id=NEW.period_id FOR UPDATE;
 IF b.id IS NULL OR b.source_authority<>'native' OR p.id IS NULL OR p.state<>'open' OR NEW.entry_date<p.start_date OR NEW.entry_date>p.end_date THEN
  RAISE EXCEPTION 'An open native book period is required' USING ERRCODE='23514';END IF;
 IF TG_OP='INSERT' THEN
  IF NEW.state<>'draft' OR NEW.revision<>1 OR NEW.posted_by IS NOT NULL OR NEW.posted_at IS NOT NULL THEN
   RAISE EXCEPTION 'Journal must be drafted before posting' USING ERRCODE='23514';END IF;
 ELSE
  IF OLD.state<>'draft' OR NEW.state NOT IN ('posted','void') OR NEW.revision<>OLD.revision+1 OR (to_jsonb(NEW)-ARRAY['state','revision','posted_by','posted_at']) IS DISTINCT FROM (to_jsonb(OLD)-ARRAY['state','revision','posted_by','posted_at']) THEN
   RAISE EXCEPTION 'Journal cannot be amended; use a reversal' USING ERRCODE='23514';END IF;
  IF NEW.state='posted' AND b.independent_posting AND NEW.posted_by=NEW.created_by THEN
   RAISE EXCEPTION 'Independent posting is required' USING ERRCODE='23514';END IF;
 END IF;RETURN NEW;
END $$;
CREATE TRIGGER journal_guard BEFORE INSERT OR UPDATE OR DELETE ON archava_accounting.journals FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_journal();
CREATE FUNCTION archava_accounting.guard_line() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE a archava_accounting.accounts%ROWTYPE;j archava_accounting.journals%ROWTYPE;
BEGIN
 SELECT * INTO j FROM archava_accounting.journals WHERE tenant_id=NEW.tenant_id AND id=NEW.journal_id FOR UPDATE;
 SELECT * INTO a FROM archava_accounting.accounts WHERE tenant_id=NEW.tenant_id AND id=NEW.account_id AND book_id=NEW.book_id FOR SHARE;
 IF j.id IS NULL OR j.state<>'draft' OR j.book_id<>NEW.book_id OR a.id IS NULL OR NOT a.active OR a.revision<>NEW.account_revision OR a.code<>NEW.account_code OR a.name<>NEW.account_name OR a.class<>NEW.class OR a.cash_role<>NEW.cash_role THEN
  RAISE EXCEPTION 'Line differs from the reviewed active account' USING ERRCODE='23514';END IF;
 -- A completed draft cannot acquire more lines even while it remains unposted.
 IF EXISTS(SELECT 1 FROM archava_accounting.audit WHERE tenant_id=NEW.tenant_id AND resource_kind='journal' AND resource_id=NEW.journal_id) THEN
  RAISE EXCEPTION 'Reviewed journal lines are immutable' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER line_guard BEFORE INSERT ON archava_accounting.journal_lines FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_line();
CREATE FUNCTION archava_accounting.assert_resource() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE kind text:=TG_ARGV[0];b uuid;events integer;current_revision integer;current_snapshot jsonb;held_snapshot jsonb;bound boolean;line_count integer;debits numeric;credits numeric;j archava_accounting.journals%ROWTYPE;
BEGIN
 EXECUTE format('SELECT to_jsonb(r),r.revision FROM archava_accounting.%I r WHERE tenant_id=$1 AND id=$2',TG_TABLE_NAME) INTO current_snapshot,current_revision USING NEW.tenant_id,NEW.id;
 b:=CASE WHEN kind='book' THEN NEW.id ELSE (current_snapshot->>'book_id')::uuid END;
 SELECT count(*) INTO events FROM archava_accounting.audit WHERE tenant_id=NEW.tenant_id AND resource_kind=kind AND resource_id=NEW.id;
 SELECT a.snapshot,c.actor_id=a.actor_id AND c.receipt->>'requestId'=a.request_id::text AND c.receipt->>'resourceKind'=kind AND c.receipt->>'resourceId'=NEW.id::text AND c.receipt->>'bookId'=b::text AND c.receipt->>'revision'=current_revision::text AND c.receipt->>'type'=a.operation
 INTO held_snapshot,bound FROM archava_accounting.audit a JOIN archava_accounting.commands c ON c.tenant_id=a.tenant_id AND c.request_id=a.request_id
 WHERE a.tenant_id=NEW.tenant_id AND a.resource_kind=kind AND a.resource_id=NEW.id AND a.revision=current_revision AND a.book_id=b;
 IF events<>current_revision OR held_snapshot->'record' IS DISTINCT FROM current_snapshot OR bound IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Financial audit and receipt are incomplete' USING ERRCODE='23514';END IF;
 IF kind='journal' THEN
  SELECT * INTO j FROM archava_accounting.journals WHERE tenant_id=NEW.tenant_id AND id=NEW.id;
  SELECT count(*),coalesce(sum(debit_minor),0),coalesce(sum(credit_minor),0) INTO line_count,debits,credits FROM archava_accounting.journal_lines WHERE tenant_id=j.tenant_id AND journal_id=j.id;
  IF line_count<2 OR line_count>100 OR debits<=0 OR debits<>credits OR held_snapshot->'lines' IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(l) ORDER BY ordinal) FROM archava_accounting.journal_lines l WHERE tenant_id=j.tenant_id AND journal_id=j.id) THEN
   RAISE EXCEPTION 'Journal lines must be balanced, complete and audited' USING ERRCODE='23514';END IF;
  IF j.state='posted' AND EXISTS(SELECT 1 FROM archava_accounting.journal_lines l JOIN archava_accounting.accounts a ON a.tenant_id=l.tenant_id AND a.id=l.account_id WHERE l.tenant_id=j.tenant_id AND l.journal_id=j.id AND (NOT a.active OR a.revision<>l.account_revision)) THEN
   RAISE EXCEPTION 'A reviewed account changed before posting' USING ERRCODE='23514';END IF;
  IF j.reversal_of IS NOT NULL THEN
   IF NOT EXISTS(SELECT 1 FROM archava_accounting.journals WHERE tenant_id=j.tenant_id AND id=j.reversal_of AND book_id=j.book_id AND state='posted' AND reversal_of IS NULL) OR EXISTS(
    (SELECT ordinal,account_id,debit_minor,credit_minor FROM archava_accounting.journal_lines WHERE tenant_id=j.tenant_id AND journal_id=j.id EXCEPT SELECT ordinal,account_id,credit_minor,debit_minor FROM archava_accounting.journal_lines WHERE tenant_id=j.tenant_id AND journal_id=j.reversal_of)
    UNION ALL
    (SELECT ordinal,account_id,credit_minor,debit_minor FROM archava_accounting.journal_lines WHERE tenant_id=j.tenant_id AND journal_id=j.reversal_of EXCEPT SELECT ordinal,account_id,debit_minor,credit_minor FROM archava_accounting.journal_lines WHERE tenant_id=j.tenant_id AND journal_id=j.id)
   ) THEN RAISE EXCEPTION 'Reversal must invert the original posted journal' USING ERRCODE='23514';END IF;
  END IF;
 END IF;RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER book_complete AFTER INSERT ON archava_accounting.books DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_accounting.assert_resource('book');
CREATE CONSTRAINT TRIGGER account_complete AFTER INSERT OR UPDATE ON archava_accounting.accounts DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_accounting.assert_resource('account');
CREATE CONSTRAINT TRIGGER period_complete AFTER INSERT OR UPDATE ON archava_accounting.periods DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_accounting.assert_resource('period');
CREATE CONSTRAINT TRIGGER journal_complete AFTER INSERT OR UPDATE ON archava_accounting.journals DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_accounting.assert_resource('journal');
DO $$ DECLARE owned text;BEGIN FOREACH owned IN ARRAY ARRAY['memberships','books','accounts','periods','journals','journal_lines','commands','audit'] LOOP
 EXECUTE format('ALTER TABLE archava_accounting.%I ENABLE ROW LEVEL SECURITY',owned);
 EXECUTE format('ALTER TABLE archava_accounting.%I FORCE ROW LEVEL SECURITY',owned);
 EXECUTE format('CREATE POLICY accounting_tenant ON archava_accounting.%I USING(tenant_id=current_setting(''archava.tenant_id'',true)) WITH CHECK(tenant_id=current_setting(''archava.tenant_id'',true))',owned);
END LOOP;END $$;
CREATE FUNCTION archava_accounting.lock_membership(wanted_tenant text,wanted_principal text) RETURNS SETOF archava_accounting.memberships LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog AS $$
 SELECT * FROM archava_accounting.memberships WHERE tenant_id=wanted_tenant AND principal_id=wanted_principal AND wanted_tenant=current_setting('archava.tenant_id',true) FOR SHARE
$$;
REVOKE ALL ON FUNCTION archava_accounting.lock_membership(text,text) FROM PUBLIC;
