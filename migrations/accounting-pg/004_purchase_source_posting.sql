-- Explicit future-only confirmed-PO purchase capture; no legacy opening import.
ALTER TABLE archava_accounting.audit DROP CONSTRAINT audit_resource_kind_check;
ALTER TABLE archava_accounting.audit ADD CONSTRAINT audit_resource_kind_check CHECK(resource_kind IN ('book','account','period','journal','invoice_policy','purchase_policy'));
CREATE TABLE archava_accounting.purchase_policies (
 tenant_id text NOT NULL,id uuid NOT NULL,book_id uuid NOT NULL,
 source_unit text NOT NULL CHECK(source_unit='whole_idr'),currency_catalogue text NOT NULL CHECK(currency_catalogue='SIX-2026-09-17'),
 mappings jsonb NOT NULL CHECK(jsonb_typeof(mappings)='array' AND jsonb_array_length(mappings)=4),
 revision integer NOT NULL CHECK(revision>0),created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,book_id),UNIQUE(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,book_id) REFERENCES archava_accounting.books(tenant_id,id)
);
CREATE TABLE archava_accounting.purchase_origins (
 tenant_id text NOT NULL,id uuid NOT NULL,book_id uuid NOT NULL,policy_id uuid NOT NULL,policy_revision integer NOT NULL CHECK(policy_revision>0),policy_snapshot jsonb NOT NULL,
 order_id uuid NOT NULL,order_revision integer NOT NULL CHECK(order_revision>0),actor_id text NOT NULL,occurred_at timestamptz NOT NULL,
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,order_id),UNIQUE(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,book_id,policy_id) REFERENCES archava_accounting.purchase_policies(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,id) REFERENCES archava_purchasing.commands(tenant_id,request_id),
 FOREIGN KEY(tenant_id,order_id) REFERENCES archava_purchasing.orders(tenant_id,id)
);
CREATE TABLE archava_accounting.purchase_sources (
 tenant_id text NOT NULL,id uuid NOT NULL,book_id uuid NOT NULL,origin_id uuid NOT NULL,
 order_id uuid NOT NULL,document_id uuid,source_revision integer NOT NULL CHECK(source_revision>0),effect text NOT NULL CHECK(effect IN ('receipt','bill','void')),
 goods_minor bigint NOT NULL CHECK(goods_minor BETWEEN -9000000000000000 AND 9000000000000000),
 landed_fee_minor bigint NOT NULL CHECK(landed_fee_minor BETWEEN 0 AND 9000000000000000),
 source_snapshot jsonb NOT NULL,actor_id text NOT NULL,occurred_at timestamptz NOT NULL,source_date date NOT NULL,
 CHECK((effect='receipt' AND goods_minor>=0 AND document_id IS NULL AND goods_minor::numeric+landed_fee_minor<=9000000000000000) OR (effect='bill' AND goods_minor>=0 AND landed_fee_minor=0 AND document_id IS NOT NULL) OR (effect='void' AND goods_minor<=0 AND landed_fee_minor=0 AND document_id IS NOT NULL)),
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,book_id,origin_id) REFERENCES archava_accounting.purchase_origins(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,id) REFERENCES archava_purchasing.commands(tenant_id,request_id),
 FOREIGN KEY(tenant_id,order_id) REFERENCES archava_purchasing.orders(tenant_id,id),
 FOREIGN KEY(tenant_id,document_id) REFERENCES archava_purchasing.supplier_documents(tenant_id,id)
);
CREATE UNIQUE INDEX one_purchase_document_effect ON archava_accounting.purchase_sources(tenant_id,document_id,source_revision) WHERE document_id IS NOT NULL;
CREATE INDEX purchase_source_book ON archava_accounting.purchase_sources(tenant_id,book_id,source_date,id);
CREATE TABLE archava_accounting.purchase_journal_sources (
 tenant_id text NOT NULL,book_id uuid NOT NULL,journal_id uuid NOT NULL,source_id uuid NOT NULL,
 PRIMARY KEY(tenant_id,journal_id),
 FOREIGN KEY(tenant_id,book_id,journal_id) REFERENCES archava_accounting.journals(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,book_id,source_id) REFERENCES archava_accounting.purchase_sources(tenant_id,book_id,id)
);
CREATE INDEX purchase_journal_source_lookup ON archava_accounting.purchase_journal_sources(tenant_id,source_id);
CREATE TRIGGER purchase_origin_immutable BEFORE UPDATE OR DELETE ON archava_accounting.purchase_origins FOR EACH ROW EXECUTE FUNCTION archava_accounting.immutable();
CREATE TRIGGER purchase_source_immutable BEFORE UPDATE OR DELETE ON archava_accounting.purchase_sources FOR EACH ROW EXECUTE FUNCTION archava_accounting.immutable();
CREATE TRIGGER purchase_journal_source_immutable BEFORE UPDATE OR DELETE ON archava_accounting.purchase_journal_sources FOR EACH ROW EXECUTE FUNCTION archava_accounting.immutable();
-- Always lock the company even if no book/policy exists yet. Policy setup must
-- call the same helper before taking any SHARE/UPDATE lock on its book.
CREATE FUNCTION archava_accounting.lock_purchase_company(wanted_tenant text,wanted_company uuid) RETURNS void LANGUAGE sql VOLATILE SET search_path=pg_catalog AS $$
 SELECT pg_advisory_xact_lock(hashtextextended('archava.purchase-origin:'||length(wanted_tenant)::text||':'||wanted_tenant||':'||wanted_company::text,0))
$$;
CREATE FUNCTION archava_accounting.prepare_purchase_confirmation() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF OLD.confirmed_at IS NULL AND NEW.confirmed_at IS NOT NULL AND NEW.company_id IS NOT NULL THEN
  PERFORM archava_accounting.lock_purchase_company(NEW.tenant_id,NEW.company_id);
  PERFORM 1 FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND company_id=NEW.company_id FOR UPDATE;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER aa_purchase_confirmation_source BEFORE UPDATE ON archava_purchasing.orders FOR EACH ROW EXECUTE FUNCTION archava_accounting.prepare_purchase_confirmation();
CREATE FUNCTION archava_accounting.guard_purchase_origin() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE b archava_accounting.books;p archava_accounting.purchase_policies;o archava_purchasing.orders;c archava_purchasing.commands;a archava_purchasing.audit;held jsonb;reviewed_at timestamptz;
BEGIN
 SELECT * INTO b FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 SELECT * INTO p FROM archava_accounting.purchase_policies WHERE tenant_id=NEW.tenant_id AND id=NEW.policy_id AND book_id=NEW.book_id;
 SELECT * INTO o FROM archava_purchasing.orders WHERE tenant_id=NEW.tenant_id AND id=NEW.order_id;
 SELECT * INTO c FROM archava_purchasing.commands WHERE tenant_id=NEW.tenant_id AND request_id=NEW.id;
 SELECT * INTO a FROM archava_purchasing.audit WHERE tenant_id=NEW.tenant_id AND request_id=NEW.id;
 SELECT snapshot->'record',occurred_at INTO held,reviewed_at FROM archava_accounting.audit WHERE tenant_id=NEW.tenant_id AND resource_kind='purchase_policy' AND resource_id=NEW.policy_id AND revision=NEW.policy_revision;
 IF b.id IS NULL OR b.source_authority<>'native' OR b.currency<>'IDR' OR b.minor_units<>2 OR o.id IS NULL OR o.currency<>'IDR' OR o.company_id IS DISTINCT FROM b.company_id OR o.confirmed_at IS NULL
 OR p.id IS NULL OR p.revision IS DISTINCT FROM NEW.policy_revision OR held IS NULL OR (held-'created_at') IS DISTINCT FROM (to_jsonb(p)-'created_at') OR (held->>'created_at')::timestamptz IS DISTINCT FROM p.created_at OR held IS DISTINCT FROM NEW.policy_snapshot
 OR a.request_id IS NULL OR a.operation<>'order.confirm' OR a.record_id IS DISTINCT FROM NEW.order_id OR a.revision IS DISTINCT FROM NEW.order_revision OR a.actor_id IS DISTINCT FROM NEW.actor_id OR a.occurred_at IS DISTINCT FROM NEW.occurred_at OR a.occurred_at<reviewed_at
 OR c.actor_id IS DISTINCT FROM NEW.actor_id OR c.receipt->>'operation' IS DISTINCT FROM 'order.confirm' OR c.receipt->>'id' IS DISTINCT FROM NEW.order_id::text OR c.receipt->>'revision' IS DISTINCT FROM NEW.order_revision::text THEN
  RAISE EXCEPTION 'Purchase origin differs from its confirmed order and reviewed current policy' USING ERRCODE='23514';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER purchase_origin_guard BEFORE INSERT ON archava_accounting.purchase_origins FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_purchase_origin();
-- Server-derived effect including the immutable operational receipt/version.
CREATE FUNCTION archava_accounting.purchase_effect(wanted_tenant text,wanted_request uuid) RETURNS jsonb LANGUAGE plpgsql VOLATILE SET search_path=pg_catalog AS $$
DECLARE c archava_purchasing.commands;a archava_purchasing.audit;r archava_purchasing.receipts;v archava_purchasing.supplier_document_versions;prior archava_purchasing.supplier_document_versions;goods numeric;fee numeric;stock numeric;quantity numeric;expected_quantity numeric;snapshot jsonb;effect text;po uuid;doc uuid;
BEGIN
 SELECT * INTO c FROM archava_purchasing.commands WHERE tenant_id=wanted_tenant AND request_id=wanted_request;
 SELECT * INTO a FROM archava_purchasing.audit WHERE tenant_id=wanted_tenant AND request_id=wanted_request;
 IF c.receipt IS NULL OR a.request_id IS NULL OR c.receipt->>'requestId' IS DISTINCT FROM wanted_request::text OR c.receipt->>'operation' IS DISTINCT FROM a.operation OR c.receipt->>'id' IS DISTINCT FROM a.record_id::text OR c.receipt->>'revision' IS DISTINCT FROM a.revision::text OR c.actor_id IS DISTINCT FROM a.actor_id THEN RETURN NULL;END IF;
 IF a.operation='order.receive' THEN
  SELECT * INTO r FROM archava_purchasing.receipts WHERE tenant_id=wanted_tenant AND request_id=wanted_request;
  IF r.request_id IS NULL OR r.order_id IS DISTINCT FROM a.record_id OR r.revision IS DISTINCT FROM a.revision OR r.actor_id IS DISTINCT FROM a.actor_id THEN RAISE EXCEPTION 'Missing immutable purchase receipt' USING ERRCODE='23514';END IF;
  SELECT coalesce(sum(l.unit_cost::numeric*x.accepted),0),coalesce(sum(x.accepted),0),jsonb_build_object('receipt',to_jsonb(r),'lines',coalesce(jsonb_agg(jsonb_build_object('receiptLine',to_jsonb(x),'unitCostWholeIdr',l.unit_cost) ORDER BY x.line_id),'[]'::jsonb))
  INTO goods,expected_quantity,snapshot FROM archava_purchasing.receipt_lines x JOIN archava_purchasing.lines l ON l.tenant_id=x.tenant_id AND l.id=x.line_id AND l.order_id=x.order_id WHERE x.tenant_id=wanted_tenant AND x.request_id=wanted_request;
  fee:=r.landed_fee_minor;
  SELECT coalesce(sum(total_cost_minor::numeric),0),coalesce(sum(original_quantity::numeric),0) INTO stock,quantity FROM archava_commerce.inventory_layers WHERE tenant_id=wanted_tenant AND source_request_id=wanted_request AND source_kind='purchase';
  IF stock IS DISTINCT FROM goods+fee OR quantity IS DISTINCT FROM expected_quantity OR EXISTS(SELECT 1 FROM archava_commerce.inventory_layers WHERE tenant_id=wanted_tenant AND source_request_id=wanted_request AND source_kind='purchase' AND total_cost_minor IS NULL) THEN RAISE EXCEPTION 'Purchase source differs from original accepted acquisition costs' USING ERRCODE='23514';END IF;
  po:=r.order_id;effect:='receipt';
 ELSIF a.operation IN ('document.match','document.void') THEN
  SELECT * INTO v FROM archava_purchasing.supplier_document_versions WHERE tenant_id=wanted_tenant AND request_id=wanted_request AND document_id=a.record_id AND revision=a.revision;
  IF v.document_id IS NULL THEN RAISE EXCEPTION 'Missing immutable supplier document version' USING ERRCODE='23514';END IF;
  IF a.operation='document.match' AND v.snapshot->>'state'='matched' THEN
   effect:='bill';goods:=(v.snapshot->>'declared_total')::numeric;
   IF NOT EXISTS(SELECT 1 FROM archava_purchasing.supplier_document_allocations WHERE tenant_id=wanted_tenant AND document_id=v.document_id AND document_revision=v.revision) OR EXISTS(
    SELECT 1 FROM archava_purchasing.supplier_document_allocations x LEFT JOIN archava_accounting.purchase_sources s ON s.tenant_id=x.tenant_id AND s.id=x.receipt_request_id AND s.effect='receipt' AND s.order_id=x.order_id WHERE x.tenant_id=wanted_tenant AND x.document_id=v.document_id AND x.document_revision=v.revision AND s.id IS NULL) THEN RAISE EXCEPTION 'Supplier claim requires captured accepted goods from this origin' USING ERRCODE='23514';END IF;
  ELSIF a.operation='document.void' AND v.snapshot->>'state'='void' THEN
   SELECT * INTO prior FROM archava_purchasing.supplier_document_versions WHERE tenant_id=wanted_tenant AND document_id=v.document_id AND revision=v.revision-1;
   IF prior.document_id IS NULL OR prior.snapshot->>'state'<>'matched' THEN RETURN NULL;END IF;
   IF NOT EXISTS(SELECT 1 FROM archava_accounting.purchase_sources s WHERE s.tenant_id=wanted_tenant AND s.id=prior.request_id AND s.effect='bill') THEN RAISE EXCEPTION 'Matched supplier claim has no captured payable source' USING ERRCODE='23514';END IF;
   effect:='void';goods:=-(prior.snapshot->>'declared_total')::numeric;
  ELSE RETURN NULL;
  END IF;
  doc:=v.document_id;po:=(v.snapshot->>'order_id')::uuid;fee:=0;
  SELECT jsonb_build_object('version',to_jsonb(v),'priorMatchedVersion',CASE WHEN effect='void' THEN to_jsonb(prior) ELSE NULL END,'allocations',coalesce(jsonb_agg(to_jsonb(x) ORDER BY x.receipt_request_id,x.line_id),'[]'::jsonb)) INTO snapshot FROM archava_purchasing.supplier_document_allocations x WHERE x.tenant_id=wanted_tenant AND x.document_id=v.document_id AND x.document_revision=CASE WHEN effect='void' THEN prior.revision ELSE v.revision END;
 ELSE RETURN NULL;
 END IF;
 IF goods IS NULL OR fee IS NULL OR abs(goods*100)>9000000000000000 OR fee*100>9000000000000000 OR (effect='receipt' AND (goods+fee)*100>9000000000000000) THEN RAISE EXCEPTION 'Purchase source minor-unit bound exceeded' USING ERRCODE='23514';END IF;
 RETURN jsonb_build_object('orderId',po,'documentId',doc,'revision',a.revision,'effect',effect,'goodsMinor',(goods*100)::bigint::text,'landedFeeMinor',(fee*100)::bigint::text,'actorId',a.actor_id,'occurredAt',a.occurred_at,'sourceDate',(a.occurred_at AT TIME ZONE 'UTC')::date,'snapshot',snapshot);
END $$;
CREATE FUNCTION archava_accounting.guard_purchase_source() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE expected jsonb;o archava_accounting.purchase_origins;b archava_accounting.books;
BEGIN
 SELECT * INTO b FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 SELECT * INTO o FROM archava_accounting.purchase_origins WHERE tenant_id=NEW.tenant_id AND id=NEW.origin_id AND book_id=NEW.book_id;
 expected:=archava_accounting.purchase_effect(NEW.tenant_id,NEW.id);
 IF o.id IS NULL OR b.id IS NULL OR o.order_id IS DISTINCT FROM NEW.order_id OR expected IS NULL
 OR expected IS DISTINCT FROM jsonb_build_object('orderId',NEW.order_id,'documentId',NEW.document_id,'revision',NEW.source_revision,'effect',NEW.effect,'goodsMinor',NEW.goods_minor::text,'landedFeeMinor',NEW.landed_fee_minor::text,'actorId',NEW.actor_id,'occurredAt',NEW.occurred_at,'sourceDate',NEW.source_date,'snapshot',NEW.source_snapshot) THEN RAISE EXCEPTION 'Purchase source differs from its immutable operating effect' USING ERRCODE='23514';END IF;
 IF EXISTS(SELECT 1 FROM archava_accounting.periods WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND state='closed' AND NEW.source_date BETWEEN start_date AND end_date) THEN RAISE EXCEPTION 'Reopen the financial source period before changing this purchase' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER purchase_source_guard BEFORE INSERT ON archava_accounting.purchase_sources FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_purchase_source();
CREATE FUNCTION archava_accounting.capture_purchase_source() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE po uuid;company uuid;b archava_accounting.books;p archava_accounting.purchase_policies;o archava_accounting.purchase_origins;a archava_purchasing.audit;e jsonb;held jsonb;
BEGIN
 IF OLD.receipt IS NOT NULL OR NEW.receipt IS NULL THEN RETURN NEW;END IF;
 SELECT * INTO a FROM archava_purchasing.audit WHERE tenant_id=NEW.tenant_id AND request_id=NEW.request_id;
 IF a.operation='order.confirm' THEN
  SELECT company_id INTO company FROM archava_purchasing.orders WHERE tenant_id=NEW.tenant_id AND id=a.record_id;
  IF company IS NULL THEN RETURN NEW;END IF;
  SELECT * INTO b FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND company_id=company FOR UPDATE;
  IF b.id IS NULL THEN RETURN NEW;END IF;
  SELECT * INTO p FROM archava_accounting.purchase_policies WHERE tenant_id=NEW.tenant_id AND book_id=b.id;
  SELECT snapshot->'record' INTO held FROM archava_accounting.audit WHERE tenant_id=NEW.tenant_id AND resource_kind='purchase_policy' AND resource_id=p.id AND revision=p.revision;
  IF p.id IS NOT NULL THEN INSERT INTO archava_accounting.purchase_origins(tenant_id,id,book_id,policy_id,policy_revision,policy_snapshot,order_id,order_revision,actor_id,occurred_at) VALUES(NEW.tenant_id,NEW.request_id,b.id,p.id,p.revision,held,a.record_id,a.revision,a.actor_id,a.occurred_at);END IF;
  RETURN NEW;
 ELSIF a.operation='order.receive' THEN po:=a.record_id;
 ELSIF a.operation IN ('document.match','document.void') THEN SELECT order_id INTO po FROM archava_purchasing.supplier_documents WHERE tenant_id=NEW.tenant_id AND id=a.record_id;
 ELSE RETURN NEW;
 END IF;
 SELECT * INTO o FROM archava_accounting.purchase_origins WHERE tenant_id=NEW.tenant_id AND order_id=po;
 -- No implicit import of orders confirmed before explicit purchase policy setup.
 IF o.id IS NULL THEN RETURN NEW;END IF;
 PERFORM 1 FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=o.book_id FOR UPDATE;
 e:=archava_accounting.purchase_effect(NEW.tenant_id,NEW.request_id);
 IF e IS NULL THEN RETURN NEW;END IF;
 INSERT INTO archava_accounting.purchase_sources(tenant_id,id,book_id,origin_id,order_id,document_id,source_revision,effect,goods_minor,landed_fee_minor,source_snapshot,actor_id,occurred_at,source_date)
 VALUES(NEW.tenant_id,NEW.request_id,o.book_id,o.id,po,(e->>'documentId')::uuid,(e->>'revision')::integer,e->>'effect',(e->>'goodsMinor')::bigint,(e->>'landedFeeMinor')::bigint,e->'snapshot',e->>'actorId',(e->>'occurredAt')::timestamptz,(e->>'sourceDate')::date);
 RETURN NEW;
END $$;
CREATE TRIGGER capture_financial_purchase_source AFTER UPDATE OF receipt ON archava_purchasing.commands FOR EACH ROW EXECUTE FUNCTION archava_accounting.capture_purchase_source();
DO $$ DECLARE owned text;BEGIN FOREACH owned IN ARRAY ARRAY['purchase_policies','purchase_origins','purchase_sources','purchase_journal_sources'] LOOP
 EXECUTE format('ALTER TABLE archava_accounting.%I ENABLE ROW LEVEL SECURITY',owned);
 EXECUTE format('ALTER TABLE archava_accounting.%I FORCE ROW LEVEL SECURITY',owned);
 EXECUTE format('CREATE POLICY accounting_tenant ON archava_accounting.%I USING(tenant_id=current_setting(''archava.tenant_id'',true)) WITH CHECK(tenant_id=current_setting(''archava.tenant_id'',true))',owned);
END LOOP;END $$;

CREATE FUNCTION archava_accounting.guard_purchase_policy() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE b archava_accounting.books; m jsonb; a archava_accounting.accounts; wanted_class text;wanted_cash text;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Purchase posting policy history cannot be deleted' USING ERRCODE='23514';END IF;
 SELECT * INTO b FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 IF b.id IS NULL OR b.source_authority<>'native' OR b.currency<>'IDR' OR b.minor_units<>2 OR b.currency_catalogue<>NEW.currency_catalogue THEN
  RAISE EXCEPTION 'This source bridge requires an explicitly reviewed IDR native book' USING ERRCODE='23514';END IF;
 IF TG_OP='INSERT' THEN
  IF NEW.revision<>1 THEN RAISE EXCEPTION 'Initial policy revision refused' USING ERRCODE='23514';END IF;
 ELSE
  IF (to_jsonb(NEW)-ARRAY['revision','mappings']) IS DISTINCT FROM (to_jsonb(OLD)-ARRAY['revision','mappings']) OR NEW.revision<>OLD.revision+1 THEN
   RAISE EXCEPTION 'Policy identity and unit basis are immutable' USING ERRCODE='23514';END IF;
 END IF;
 IF (SELECT count(DISTINCT v->>'role') FROM jsonb_array_elements(NEW.mappings) v)<>4 OR
    (SELECT count(DISTINCT v->>'accountId') FROM jsonb_array_elements(NEW.mappings) v)<>4 THEN
  RAISE EXCEPTION 'Four distinct source roles/accounts are required' USING ERRCODE='23514';END IF;
 FOR m IN SELECT * FROM jsonb_array_elements(NEW.mappings) LOOP
  IF m->>'role' NOT IN ('inventory','goods_received_not_invoiced','payable','landed_fee_accrual') OR NOT(m ?& ARRAY['role','accountId','expectedAccountRevision']) OR jsonb_typeof(m->'expectedAccountRevision') IS DISTINCT FROM 'number' OR (m-ARRAY['role','accountId','expectedAccountRevision'])<>'{}'::jsonb THEN
   RAISE EXCEPTION 'Unknown source account mapping' USING ERRCODE='23514';END IF;
  wanted_class:=CASE WHEN m->>'role'='inventory' THEN 'asset' ELSE 'liability' END;
  wanted_cash:='operating';
  SELECT * INTO a FROM archava_accounting.accounts WHERE tenant_id=NEW.tenant_id AND book_id=b.id AND id=(m->>'accountId')::uuid FOR SHARE;
  IF a.id IS NULL OR NOT a.active OR a.revision IS DISTINCT FROM (m->>'expectedAccountRevision')::integer OR a.class<>wanted_class OR a.cash_role<>wanted_cash THEN
   RAISE EXCEPTION 'A source account differs from its reviewed classification/revision' USING ERRCODE='23514';END IF;
 END LOOP;
 RETURN NEW;
END $$;
CREATE TRIGGER purchase_policy_guard BEFORE INSERT OR UPDATE OR DELETE ON archava_accounting.purchase_policies FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_purchase_policy();
CREATE CONSTRAINT TRIGGER purchase_policy_complete AFTER INSERT OR UPDATE ON archava_accounting.purchase_policies DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_accounting.assert_resource('purchase_policy');


-- Captured effects are reviewed into a separate ordinary journal draft.
CREATE FUNCTION archava_accounting.purchase_legs(wanted_tenant text,wanted_source uuid) RETURNS TABLE(role text,debit_minor bigint,credit_minor bigint) LANGUAGE sql STABLE SET search_path=pg_catalog AS $$
 WITH s AS(SELECT * FROM archava_accounting.purchase_sources WHERE tenant_id=wanted_tenant AND id=wanted_source), legs AS(
 SELECT 'inventory'::text AS role,goods_minor+landed_fee_minor AS debit_minor,0::bigint AS credit_minor FROM s WHERE effect='receipt'
 UNION ALL SELECT 'goods_received_not_invoiced',0,goods_minor FROM s WHERE effect='receipt'
 UNION ALL SELECT 'landed_fee_accrual',0,landed_fee_minor FROM s WHERE effect='receipt'
 UNION ALL SELECT 'goods_received_not_invoiced',goods_minor,0 FROM s WHERE effect='bill'
 UNION ALL SELECT 'payable',0,goods_minor FROM s WHERE effect='bill'
 UNION ALL SELECT 'payable',-goods_minor,0 FROM s WHERE effect='void'
 UNION ALL SELECT 'goods_received_not_invoiced',0,-goods_minor FROM s WHERE effect='void'
 )SELECT * FROM legs WHERE debit_minor<>0 OR credit_minor<>0
$$;
CREATE FUNCTION archava_accounting.guard_purchase_journal_source() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE s archava_accounting.purchase_sources;j archava_accounting.journals;p jsonb;expected_count integer;
BEGIN
 PERFORM 1 FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 SELECT * INTO s FROM archava_accounting.purchase_sources WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND id=NEW.source_id;
 SELECT policy_snapshot INTO p FROM archava_accounting.purchase_origins WHERE tenant_id=NEW.tenant_id AND id=s.origin_id AND book_id=NEW.book_id;
 SELECT * INTO j FROM archava_accounting.journals WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND id=NEW.journal_id;
 SELECT count(*) INTO expected_count FROM archava_accounting.purchase_legs(NEW.tenant_id,NEW.source_id);
 IF s.id IS NULL OR expected_count=0 OR p IS NULL OR j.id IS NULL OR j.state<>'draft' OR j.reversal_of IS NOT NULL OR j.entry_date<>s.source_date OR j.source_ref<>'purchasing:'||s.id::text||':'||j.code
 OR EXISTS(SELECT 1 FROM archava_accounting.purchase_journal_sources x JOIN archava_accounting.journals y ON y.tenant_id=x.tenant_id AND y.id=x.journal_id WHERE x.tenant_id=NEW.tenant_id AND x.source_id=NEW.source_id AND y.state<>'void') THEN RAISE EXCEPTION 'A purchase source requires one reviewed live source draft' USING ERRCODE='23514';END IF;
 IF (SELECT count(*) FROM archava_accounting.journal_lines WHERE tenant_id=NEW.tenant_id AND journal_id=j.id)<>expected_count OR EXISTS(
 SELECT 1 FROM archava_accounting.purchase_legs(NEW.tenant_id,NEW.source_id) l LEFT JOIN LATERAL(SELECT (v->>'accountId')::uuid AS id FROM jsonb_array_elements(p->'mappings') v WHERE v->>'role'=l.role) account ON true
 WHERE account.id IS NULL OR NOT EXISTS(SELECT 1 FROM archava_accounting.journal_lines x WHERE x.tenant_id=NEW.tenant_id AND x.journal_id=j.id AND x.account_id=account.id AND x.debit_minor=l.debit_minor AND x.credit_minor=l.credit_minor)) THEN RAISE EXCEPTION 'Purchase draft differs from captured goods, fee or account roles' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER purchase_journal_source_guard BEFORE INSERT ON archava_accounting.purchase_journal_sources FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_purchase_journal_source();
CREATE FUNCTION archava_accounting.assert_purchase_journal_claim() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE wanted uuid;j archava_accounting.journals;s uuid;op text;r jsonb;
BEGIN
 wanted:=coalesce((to_jsonb(NEW)->>'journal_id')::uuid,(to_jsonb(NEW)->>'id')::uuid);
 SELECT * INTO j FROM archava_accounting.journals WHERE tenant_id=NEW.tenant_id AND id=wanted;
 SELECT source_id INTO s FROM archava_accounting.purchase_journal_sources WHERE tenant_id=NEW.tenant_id AND journal_id=wanted;
 SELECT a.operation,c.receipt INTO op,r FROM archava_accounting.audit a JOIN archava_accounting.commands c ON c.tenant_id=a.tenant_id AND c.request_id=a.request_id WHERE a.tenant_id=NEW.tenant_id AND a.resource_kind='journal' AND a.resource_id=wanted AND a.revision=1;
 IF s IS NOT NULL THEN
  IF op IS DISTINCT FROM 'purchase_source.draft' OR r->>'sourceId' IS DISTINCT FROM s::text THEN RAISE EXCEPTION 'Purchase source journal must retain its reviewed command receipt' USING ERRCODE='23514';END IF;
 ELSIF op='purchase_source.draft' OR j.source_ref LIKE 'purchasing:%' THEN RAISE EXCEPTION 'Reserved purchasing source references require a bound source journal' USING ERRCODE='23514';END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER purchase_journal_claim_complete AFTER INSERT ON archava_accounting.journals DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_accounting.assert_purchase_journal_claim();
CREATE CONSTRAINT TRIGGER purchase_link_claim_complete AFTER INSERT ON archava_accounting.purchase_journal_sources DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_accounting.assert_purchase_journal_claim();
CREATE FUNCTION archava_accounting.purchase_close_guard() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF NEW.state='closed' AND EXISTS(SELECT 1 FROM archava_accounting.purchase_sources s WHERE s.tenant_id=NEW.tenant_id AND s.book_id=NEW.book_id AND (s.goods_minor<>0 OR s.landed_fee_minor<>0) AND s.source_date BETWEEN NEW.start_date AND NEW.end_date AND NOT EXISTS(
 SELECT 1 FROM archava_accounting.purchase_journal_sources x JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id WHERE x.tenant_id=s.tenant_id AND x.source_id=s.id AND j.state='posted')) THEN RAISE EXCEPTION 'Resolve unposted receipt, supplier claim and correction sources before closing' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER purchase_source_close_guard BEFORE UPDATE ON archava_accounting.periods FOR EACH ROW EXECUTE FUNCTION archava_accounting.purchase_close_guard();
CREATE FUNCTION archava_accounting.purchase_reversal_guard() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF NEW.reversal_of IS NOT NULL AND EXISTS(SELECT 1 FROM archava_accounting.purchase_journal_sources WHERE tenant_id=NEW.tenant_id AND journal_id=NEW.reversal_of) THEN RAISE EXCEPTION 'Correct the originating purchase document through its supported operating workflow' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER purchase_source_reversal_guard BEFORE INSERT ON archava_accounting.journals FOR EACH ROW EXECUTE FUNCTION archava_accounting.purchase_reversal_guard();

CREATE FUNCTION archava_accounting.purchase_guards_complete() RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog AS $$
 SELECT NOT EXISTS(SELECT 1 FROM (VALUES
 ('archava_purchasing','orders','aa_purchase_confirmation_source','archava_accounting.prepare_purchase_confirmation()',19,false,false,'',''),
 ('archava_purchasing','commands','capture_financial_purchase_source','archava_accounting.capture_purchase_source()',17,false,false,'5',''),
 ('archava_accounting','purchase_policies','purchase_policy_guard','archava_accounting.guard_purchase_policy()',31,false,false,'',''),
 ('archava_accounting','purchase_policies','purchase_policy_complete','archava_accounting.assert_resource()',21,true,true,'','70757263686173655f706f6c69637900'),
 ('archava_accounting','purchase_origins','purchase_origin_guard','archava_accounting.guard_purchase_origin()',7,false,false,'',''),
 ('archava_accounting','purchase_origins','purchase_origin_immutable','archava_accounting.immutable()',27,false,false,'',''),
 ('archava_accounting','purchase_sources','purchase_source_guard','archava_accounting.guard_purchase_source()',7,false,false,'',''),
 ('archava_accounting','purchase_sources','purchase_source_immutable','archava_accounting.immutable()',27,false,false,'',''),
 ('archava_accounting','purchase_journal_sources','purchase_journal_source_guard','archava_accounting.guard_purchase_journal_source()',7,false,false,'',''),
 ('archava_accounting','purchase_journal_sources','purchase_journal_source_immutable','archava_accounting.immutable()',27,false,false,'',''),
 ('archava_accounting','journals','purchase_journal_claim_complete','archava_accounting.assert_purchase_journal_claim()',5,true,true,'',''),
 ('archava_accounting','purchase_journal_sources','purchase_link_claim_complete','archava_accounting.assert_purchase_journal_claim()',5,true,true,'',''),
 ('archava_accounting','periods','purchase_source_close_guard','archava_accounting.purchase_close_guard()',19,false,false,'',''),
 ('archava_accounting','journals','purchase_source_reversal_guard','archava_accounting.purchase_reversal_guard()',7,false,false,'','')
 ) AS required(schema_name,table_name,trigger_name,function_name,trigger_type,is_deferred,initially_deferred,columns,args_hex)
 WHERE NOT EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid=to_regclass(format('%I.%I',required.schema_name,required.table_name)) AND t.tgname=required.trigger_name
  AND t.tgfoid=to_regprocedure(required.function_name) AND t.tgtype=required.trigger_type AND t.tgenabled='O' AND NOT t.tgisinternal
  AND t.tgdeferrable=required.is_deferred AND t.tginitdeferred=required.initially_deferred AND t.tgqual IS NULL
  AND t.tgattr=required.columns::int2vector AND encode(t.tgargs,'hex')=required.args_hex))
$$;
REVOKE ALL ON FUNCTION archava_accounting.purchase_guards_complete() FROM PUBLIC;
REVOKE ALL ON FUNCTION archava_accounting.lock_purchase_company(text,uuid) FROM PUBLIC;
