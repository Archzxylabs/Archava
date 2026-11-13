-- Explicit future-only whole-IDR invoice capture. No legacy invoice/opening import.
ALTER TABLE archava_accounting.audit DROP CONSTRAINT audit_resource_kind_check;
ALTER TABLE archava_accounting.audit ADD CONSTRAINT audit_resource_kind_check CHECK(resource_kind IN ('book','account','period','journal','invoice_policy'));
CREATE TABLE archava_accounting.invoice_policies (
 tenant_id text NOT NULL,id uuid NOT NULL,book_id uuid NOT NULL,
 source_unit text NOT NULL CHECK(source_unit='whole_idr'),currency_catalogue text NOT NULL CHECK(currency_catalogue='SIX-2026-09-17'),
 mappings jsonb NOT NULL CHECK(jsonb_typeof(mappings)='array' AND jsonb_array_length(mappings)=3),
 revision integer NOT NULL CHECK(revision>0),created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,book_id),UNIQUE(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,book_id) REFERENCES archava_accounting.books(tenant_id,id)
);
CREATE TABLE archava_accounting.invoice_sources (
 tenant_id text NOT NULL,id uuid NOT NULL,book_id uuid NOT NULL,policy_id uuid NOT NULL,policy_revision integer NOT NULL CHECK(policy_revision>0),
 policy_snapshot jsonb NOT NULL,invoice_id uuid NOT NULL,invoice_code text NOT NULL,invoice_revision integer NOT NULL CHECK(invoice_revision>0),
 effect text NOT NULL CHECK(effect IN ('issue','correction','void','collection','refund')),
 amount_minor bigint NOT NULL CHECK(amount_minor BETWEEN -9000000000000000 AND 9000000000000000),
 actor_id text NOT NULL,occurred_at timestamptz NOT NULL,source_date date NOT NULL,
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,book_id,policy_id) REFERENCES archava_accounting.invoice_policies(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,id) REFERENCES archava_billing.commands(tenant_id,request_id),
 FOREIGN KEY(tenant_id,invoice_id) REFERENCES archava_billing.invoices(tenant_id,id)
);
CREATE UNIQUE INDEX one_invoice_issue_source ON archava_accounting.invoice_sources(tenant_id,invoice_id) WHERE effect='issue';
CREATE INDEX invoice_source_book ON archava_accounting.invoice_sources(tenant_id,book_id,source_date,id);
CREATE TABLE archava_accounting.journal_sources (
 tenant_id text NOT NULL,book_id uuid NOT NULL,journal_id uuid NOT NULL,source_id uuid NOT NULL,
 PRIMARY KEY(tenant_id,journal_id),
 FOREIGN KEY(tenant_id,book_id,journal_id) REFERENCES archava_accounting.journals(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,book_id,source_id) REFERENCES archava_accounting.invoice_sources(tenant_id,book_id,id)
);
CREATE TRIGGER invoice_source_immutable BEFORE UPDATE OR DELETE ON archava_accounting.invoice_sources FOR EACH ROW EXECUTE FUNCTION archava_accounting.immutable();
CREATE TRIGGER journal_source_immutable BEFORE UPDATE OR DELETE ON archava_accounting.journal_sources FOR EACH ROW EXECUTE FUNCTION archava_accounting.immutable();

CREATE FUNCTION archava_accounting.guard_invoice_policy() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE b archava_accounting.books; m jsonb; a archava_accounting.accounts; wanted_class text;wanted_cash text;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Invoice posting policy history cannot be deleted' USING ERRCODE='23514';END IF;
 SELECT * INTO b FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 IF b.id IS NULL OR b.source_authority<>'native' OR b.currency<>'IDR' OR b.minor_units<>2 OR b.currency_catalogue<>NEW.currency_catalogue THEN
  RAISE EXCEPTION 'This source bridge requires an explicitly reviewed IDR native book' USING ERRCODE='23514';END IF;
 IF TG_OP='INSERT' THEN
  IF NEW.revision<>1 THEN RAISE EXCEPTION 'Initial policy revision refused' USING ERRCODE='23514';END IF;
 ELSE
  IF (to_jsonb(NEW)-ARRAY['revision','mappings']) IS DISTINCT FROM (to_jsonb(OLD)-ARRAY['revision','mappings']) OR NEW.revision<>OLD.revision+1 THEN
   RAISE EXCEPTION 'Policy identity and unit basis are immutable' USING ERRCODE='23514';END IF;
 END IF;
 IF (SELECT count(DISTINCT v->>'role') FROM jsonb_array_elements(NEW.mappings) v)<>3 OR
    (SELECT count(DISTINCT v->>'accountId') FROM jsonb_array_elements(NEW.mappings) v)<>3 THEN
  RAISE EXCEPTION 'Three distinct source roles/accounts are required' USING ERRCODE='23514';END IF;
 FOR m IN SELECT * FROM jsonb_array_elements(NEW.mappings) LOOP
  IF m->>'role' NOT IN ('receivable','revenue','payment_clearing') OR NOT(m ?& ARRAY['role','accountId','expectedAccountRevision']) OR (m-ARRAY['role','accountId','expectedAccountRevision'])<>'{}'::jsonb THEN
   RAISE EXCEPTION 'Unknown source account mapping' USING ERRCODE='23514';END IF;
  wanted_class:=CASE WHEN m->>'role'='revenue' THEN 'revenue' ELSE 'asset' END;
  wanted_cash:=CASE WHEN m->>'role'='payment_clearing' THEN 'none' ELSE 'operating' END;
  SELECT * INTO a FROM archava_accounting.accounts WHERE tenant_id=NEW.tenant_id AND book_id=b.id AND id=(m->>'accountId')::uuid FOR SHARE;
  IF a.id IS NULL OR NOT a.active OR a.revision<>(m->>'expectedAccountRevision')::integer OR a.class<>wanted_class OR a.cash_role<>wanted_cash THEN
   RAISE EXCEPTION 'A source account differs from its reviewed classification/revision' USING ERRCODE='23514';END IF;
 END LOOP;
 RETURN NEW;
END $$;
CREATE TRIGGER invoice_policy_guard BEFORE INSERT OR UPDATE OR DELETE ON archava_accounting.invoice_policies FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_invoice_policy();
CREATE CONSTRAINT TRIGGER invoice_policy_complete AFTER INSERT OR UPDATE ON archava_accounting.invoice_policies DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_accounting.assert_resource('invoice_policy');

-- Every amount is derived from the committed source receipt and its immutable child.
CREATE FUNCTION archava_accounting.invoice_effect(wanted_tenant text,wanted_request uuid) RETURNS TABLE(effect text,whole_idr numeric)
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
   SELECT amount_minor,e.outcome INTO delta,outcome FROM archava_billing.payment_events e WHERE tenant_id=wanted_tenant AND id=(r->>'eventId')::uuid AND invoice_id=(r->>'invoiceId')::uuid AND currency='IDR';
   IF outcome NOT IN ('succeeded','refunded') OR outcome IS NULL THEN RETURN;END IF;
   effect:=CASE WHEN outcome='succeeded' THEN 'collection' ELSE 'refund' END;
   whole_idr:=CASE WHEN outcome='succeeded' THEN delta ELSE -delta END;
  ELSE RETURN;
 END CASE;
 IF whole_idr IS NULL THEN RAISE EXCEPTION 'Missing immutable invoice effect' USING ERRCODE='23514';END IF;
 RETURN NEXT;
END $$;

CREATE FUNCTION archava_accounting.guard_invoice_source() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE c archava_billing.commands;i archava_billing.invoices;b archava_accounting.books;expected record;held jsonb;original archava_accounting.invoice_sources;
BEGIN
 SELECT * INTO b FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 SELECT * INTO c FROM archava_billing.commands WHERE tenant_id=NEW.tenant_id AND request_id=NEW.id;
 SELECT * INTO i FROM archava_billing.invoices WHERE tenant_id=NEW.tenant_id AND id=NEW.invoice_id;
 SELECT * INTO expected FROM archava_accounting.invoice_effect(NEW.tenant_id,NEW.id);
 SELECT snapshot->'record' INTO held FROM archava_accounting.audit WHERE tenant_id=NEW.tenant_id AND resource_kind='invoice_policy' AND resource_id=NEW.policy_id AND revision=NEW.policy_revision;
 IF b.id IS NULL OR b.source_authority<>'native' OR b.currency<>'IDR' OR b.minor_units<>2 OR i.id IS NULL OR i.company_id IS DISTINCT FROM b.company_id OR i.currency<>'IDR'
  OR c.actor_id IS DISTINCT FROM NEW.actor_id OR c.receipt->>'invoiceId' IS DISTINCT FROM NEW.invoice_id::text OR c.receipt->>'revision' IS DISTINCT FROM NEW.invoice_revision::text
  OR c.receipt->>'executedAt' IS NULL OR (c.receipt->>'executedAt')::timestamptz IS DISTINCT FROM NEW.occurred_at OR NEW.source_date IS DISTINCT FROM (NEW.occurred_at AT TIME ZONE 'UTC')::date
  OR NEW.invoice_code IS DISTINCT FROM i.invoice_code OR expected.effect IS DISTINCT FROM NEW.effect OR expected.whole_idr*100 IS DISTINCT FROM NEW.amount_minor::numeric
  OR held IS NULL OR held IS DISTINCT FROM NEW.policy_snapshot OR held->>'book_id' IS DISTINCT FROM NEW.book_id::text THEN
  RAISE EXCEPTION 'Financial source differs from its immutable invoice receipt/policy' USING ERRCODE='23514';END IF;
 IF NEW.effect<>'issue' THEN
  SELECT * INTO original FROM archava_accounting.invoice_sources WHERE tenant_id=NEW.tenant_id AND invoice_id=NEW.invoice_id AND effect='issue';
  IF original.id IS NULL OR (NEW.book_id,NEW.policy_id,NEW.policy_revision,NEW.policy_snapshot) IS DISTINCT FROM (original.book_id,original.policy_id,original.policy_revision,original.policy_snapshot) THEN
   RAISE EXCEPTION 'Invoice corrections retain their original posting policy' USING ERRCODE='23514';END IF;
 END IF;
 IF EXISTS(SELECT 1 FROM archava_accounting.periods WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND state='closed' AND NEW.source_date BETWEEN start_date AND end_date) THEN
  RAISE EXCEPTION 'Reopen the financial source period before changing its invoice' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER invoice_source_guard BEFORE INSERT ON archava_accounting.invoice_sources FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_invoice_source();

CREATE FUNCTION archava_accounting.capture_invoice_source() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE i archava_billing.invoices;b archava_accounting.books;p archava_accounting.invoice_policies;original archava_accounting.invoice_sources;expected record;held jsonb;
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
 INSERT INTO archava_accounting.invoice_sources(tenant_id,id,book_id,policy_id,policy_revision,policy_snapshot,invoice_id,invoice_code,invoice_revision,effect,amount_minor,actor_id,occurred_at,source_date)
 VALUES(NEW.tenant_id,NEW.request_id,b.id,p.id,p.revision,held,i.id,i.invoice_code,(NEW.receipt->>'revision')::integer,expected.effect,expected.whole_idr*100,NEW.actor_id,(NEW.receipt->>'executedAt')::timestamptz,((NEW.receipt->>'executedAt')::timestamptz AT TIME ZONE 'UTC')::date);
 RETURN NEW;
END $$;
CREATE TRIGGER capture_financial_invoice_source AFTER UPDATE OF receipt ON archava_billing.commands FOR EACH ROW EXECUTE FUNCTION archava_accounting.capture_invoice_source();

CREATE FUNCTION archava_accounting.guard_journal_source() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE s archava_accounting.invoice_sources;j archava_accounting.journals;ar uuid;counterpart uuid;debit uuid;credit uuid;
BEGIN
 PERFORM 1 FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 SELECT * INTO s FROM archava_accounting.invoice_sources WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND id=NEW.source_id;
 SELECT * INTO j FROM archava_accounting.journals WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND id=NEW.journal_id;
 IF s.id IS NULL OR s.amount_minor=0 OR j.id IS NULL OR j.state<>'draft' OR j.reversal_of IS NOT NULL OR j.entry_date<>s.source_date OR j.source_ref<>'billing:'||s.id::text||':'||j.code
  OR EXISTS(SELECT 1 FROM archava_accounting.journal_sources x JOIN archava_accounting.journals y ON y.tenant_id=x.tenant_id AND y.id=x.journal_id WHERE x.tenant_id=NEW.tenant_id AND x.source_id=NEW.source_id AND y.state<>'void') THEN
  RAISE EXCEPTION 'A financial source needs one reviewed live draft' USING ERRCODE='23514';END IF;
 SELECT (v->>'accountId')::uuid INTO ar FROM jsonb_array_elements(s.policy_snapshot->'mappings') v WHERE v->>'role'='receivable';
 SELECT (v->>'accountId')::uuid INTO counterpart FROM jsonb_array_elements(s.policy_snapshot->'mappings') v WHERE v->>'role'=CASE WHEN s.effect IN ('collection','refund') THEN 'payment_clearing' ELSE 'revenue' END;
 debit:=CASE WHEN s.effect IN ('collection','refund') THEN counterpart ELSE ar END;
 credit:=CASE WHEN s.effect IN ('collection','refund') THEN ar ELSE counterpart END;
 IF s.amount_minor<0 THEN ar:=debit;debit:=credit;credit:=ar;END IF;
 IF (SELECT count(*) FROM archava_accounting.journal_lines WHERE tenant_id=NEW.tenant_id AND journal_id=j.id)<>2 OR NOT EXISTS(
  SELECT 1 FROM archava_accounting.journal_lines WHERE tenant_id=NEW.tenant_id AND journal_id=j.id AND account_id=debit AND debit_minor=abs(s.amount_minor) AND credit_minor=0) OR NOT EXISTS(
  SELECT 1 FROM archava_accounting.journal_lines WHERE tenant_id=NEW.tenant_id AND journal_id=j.id AND account_id=credit AND credit_minor=abs(s.amount_minor) AND debit_minor=0) THEN
  RAISE EXCEPTION 'Source draft must use the exact captured amount and account roles' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER journal_source_guard BEFORE INSERT ON archava_accounting.journal_sources FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_journal_source();
CREATE FUNCTION archava_accounting.source_close_guard() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF NEW.state='closed' AND EXISTS(SELECT 1 FROM archava_accounting.invoice_sources s WHERE s.tenant_id=NEW.tenant_id AND s.book_id=NEW.book_id AND s.amount_minor<>0 AND s.source_date BETWEEN NEW.start_date AND NEW.end_date AND NOT EXISTS(
  SELECT 1 FROM archava_accounting.journal_sources x JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id WHERE x.tenant_id=s.tenant_id AND x.source_id=s.id AND j.state='posted')) THEN
  RAISE EXCEPTION 'Resolve unposted invoice sources before closing' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER invoice_source_close_guard BEFORE UPDATE ON archava_accounting.periods FOR EACH ROW EXECUTE FUNCTION archava_accounting.source_close_guard();
CREATE FUNCTION archava_accounting.source_reversal_guard() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF NEW.reversal_of IS NOT NULL AND EXISTS(SELECT 1 FROM archava_accounting.journal_sources WHERE tenant_id=NEW.tenant_id AND journal_id=NEW.reversal_of) THEN
  RAISE EXCEPTION 'Correct the source invoice or refund through its verified source workflow' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER invoice_source_reversal_guard BEFORE INSERT ON archava_accounting.journals FOR EACH ROW EXECUTE FUNCTION archava_accounting.source_reversal_guard();
DO $$ DECLARE owned text;BEGIN FOREACH owned IN ARRAY ARRAY['invoice_policies','invoice_sources','journal_sources'] LOOP
 EXECUTE format('ALTER TABLE archava_accounting.%I ENABLE ROW LEVEL SECURITY',owned);
 EXECUTE format('ALTER TABLE archava_accounting.%I FORCE ROW LEVEL SECURITY',owned);
 EXECUTE format('CREATE POLICY accounting_tenant ON archava_accounting.%I USING(tenant_id=current_setting(''archava.tenant_id'',true)) WITH CHECK(tenant_id=current_setting(''archava.tenant_id'',true))',owned);
END LOOP;END $$;
