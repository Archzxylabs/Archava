-- Trade currency stage 4: the invoice, purchase and stock source bridges accept a
-- second source unit. `whole_idr` (factor 100) stays valid only on a native IDR
-- book with two minor units; `minor` (factor 1, the amount is already the book
-- currency's ISO minor unit, whatever its exponent) is valid only on a native
-- book whose currency is not IDR. A source whose currency differs from its
-- book's is refused, not skipped. Earlier SQL bytes are unchanged: every function
-- is replaced in place, so its OID, owner, grants and the triggers the readiness
-- functions identify by OID are kept. No row is rewritten and nothing is imported.
DO $$ BEGIN
 PERFORM pg_advisory_xact_lock(hashtext('archava-serving-writer-takeover'));
 IF to_regclass('archava_control.writer_lease') IS NOT NULL THEN
  PERFORM 1 FROM archava_control.writer_lease WHERE lease_name='archava-serving' FOR UPDATE;
 END IF;
END $$;
ALTER TABLE archava_accounting.invoice_policies DROP CONSTRAINT invoice_policies_source_unit_check,ADD CONSTRAINT invoice_policy_source_unit CHECK(source_unit IN ('whole_idr','minor'));
ALTER TABLE archava_accounting.purchase_policies DROP CONSTRAINT purchase_policies_source_unit_check,ADD CONSTRAINT purchase_policy_source_unit CHECK(source_unit IN ('whole_idr','minor'));
ALTER TABLE archava_accounting.stock_policies DROP CONSTRAINT stock_policies_source_unit_check,ADD CONSTRAINT stock_policy_source_unit CHECK(source_unit IN ('whole_idr','minor'));

-- Policy guards: the unit must fit the book. The unit stays immutable per policy
-- and the currency per book, so one book only ever has one unit.
CREATE OR REPLACE FUNCTION archava_accounting.guard_invoice_policy() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE b archava_accounting.books; m jsonb; a archava_accounting.accounts; wanted_class text;wanted_cash text;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Invoice posting policy history cannot be deleted' USING ERRCODE='23514';END IF;
 SELECT * INTO b FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 IF b.id IS NULL OR b.source_authority<>'native' OR b.currency_catalogue<>NEW.currency_catalogue
  OR ((NEW.source_unit='whole_idr' AND b.currency='IDR' AND b.minor_units=2) OR (NEW.source_unit='minor' AND b.currency<>'IDR')) IS NOT TRUE THEN
  RAISE EXCEPTION 'This source bridge requires a reviewed native book and its source unit: whole_idr on an IDR book, minor on any other' USING ERRCODE='23514';END IF;
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
CREATE OR REPLACE FUNCTION archava_accounting.guard_purchase_policy() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE b archava_accounting.books; m jsonb; a archava_accounting.accounts; wanted_class text;wanted_cash text;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Purchase posting policy history cannot be deleted' USING ERRCODE='23514';END IF;
 SELECT * INTO b FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 IF b.id IS NULL OR b.source_authority<>'native' OR b.currency_catalogue<>NEW.currency_catalogue
  OR ((NEW.source_unit='whole_idr' AND b.currency='IDR' AND b.minor_units=2) OR (NEW.source_unit='minor' AND b.currency<>'IDR')) IS NOT TRUE THEN
  RAISE EXCEPTION 'This source bridge requires a reviewed native book and its source unit: whole_idr on an IDR book, minor on any other' USING ERRCODE='23514';END IF;
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
CREATE OR REPLACE FUNCTION archava_accounting.guard_stock_policy() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE b archava_accounting.books;a archava_accounting.accounts;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Stock posting policy history cannot be deleted' USING ERRCODE='23514';END IF;
 SELECT * INTO b FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 SELECT * INTO a FROM archava_accounting.accounts WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND id=NEW.cogs_account_id FOR SHARE;
 IF b.id IS NULL OR b.source_authority<>'native' OR b.currency_catalogue<>NEW.currency_catalogue
  OR ((NEW.source_unit='whole_idr' AND b.currency='IDR' AND b.minor_units=2) OR (NEW.source_unit='minor' AND b.currency<>'IDR')) IS NOT TRUE
  OR a.id IS NULL OR NOT a.active OR a.revision<>NEW.account_revision OR a.class<>'cost_of_goods_sold' OR a.cash_role<>'operating' THEN RAISE EXCEPTION 'Stock policy requires a reviewed native book, its source unit (whole_idr on an IDR book, minor on any other) and a current COGS account' USING ERRCODE='23514';END IF;
 IF TG_OP='INSERT' THEN
  IF NEW.revision<>1 THEN RAISE EXCEPTION 'Initial stock policy revision refused' USING ERRCODE='23514';END IF;
 ELSIF (to_jsonb(NEW)-ARRAY['revision','cogs_account_id','account_revision']) IS DISTINCT FROM (to_jsonb(OLD)-ARRAY['revision','cogs_account_id','account_revision']) OR NEW.revision<>OLD.revision+1 THEN RAISE EXCEPTION 'Stock policy identity and unit basis are immutable' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;

-- Invoice bridge. A function replaced in place cannot rename its OUT columns, so
-- `whole_idr` keeps its name: it is the amount in the invoice's source unit (whole
-- Rupiah under a `whole_idr` policy, the currency's ISO minor unit under `minor`).
-- The caller applies the factor of the policy snapshot the source is captured under.
-- A provider event is matched in its invoice's currency instead of the literal IDR.
CREATE OR REPLACE FUNCTION archava_accounting.invoice_effect(wanted_tenant text,wanted_request uuid) RETURNS TABLE(effect text,whole_idr numeric)
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
   SELECT e.amount_minor,e.outcome INTO delta,outcome FROM archava_billing.payment_events e JOIN archava_billing.invoices i ON i.tenant_id=e.tenant_id AND i.id=e.invoice_id
    WHERE e.tenant_id=wanted_tenant AND e.id=(r->>'eventId')::uuid AND e.invoice_id=(r->>'invoiceId')::uuid AND e.currency=i.currency;
   IF outcome NOT IN ('succeeded','refunded') OR outcome IS NULL THEN RETURN;END IF;
   effect:=CASE WHEN outcome='succeeded' THEN 'collection' ELSE 'refund' END;
   whole_idr:=CASE WHEN outcome='succeeded' THEN delta ELSE -delta END;
  ELSE RETURN;
 END CASE;
 IF whole_idr IS NULL THEN RAISE EXCEPTION 'Missing immutable invoice effect' USING ERRCODE='23514';END IF;
 RETURN NEXT;
END $$;
CREATE OR REPLACE FUNCTION archava_accounting.guard_invoice_source() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE c archava_billing.commands;i archava_billing.invoices;b archava_accounting.books;expected record;held jsonb;original archava_accounting.invoice_sources;unit_factor numeric;
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
 IF EXISTS(SELECT 1 FROM archava_accounting.periods WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND state='closed' AND NEW.source_date BETWEEN start_date AND end_date) THEN
  RAISE EXCEPTION 'Reopen the financial source period before changing its invoice' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
-- Still quiet without a company, a book or a policy, and for an invoice issued
-- before its policy. With a policy, an invoice in another currency than the book
-- is refused here, which refuses the billing command that produced it.
CREATE OR REPLACE FUNCTION archava_accounting.capture_invoice_source() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE i archava_billing.invoices;b archava_accounting.books;p archava_accounting.invoice_policies;original archava_accounting.invoice_sources;expected record;held jsonb;unit_factor numeric;
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
 INSERT INTO archava_accounting.invoice_sources(tenant_id,id,book_id,policy_id,policy_revision,policy_snapshot,invoice_id,invoice_code,invoice_revision,effect,amount_minor,actor_id,occurred_at,source_date)
 VALUES(NEW.tenant_id,NEW.request_id,b.id,p.id,p.revision,held,i.id,i.invoice_code,(NEW.receipt->>'revision')::integer,expected.effect,expected.whole_idr*unit_factor,NEW.actor_id,(NEW.receipt->>'executedAt')::timestamptz,((NEW.receipt->>'executedAt')::timestamptz AT TIME ZONE 'UTC')::date);
 RETURN NEW;
END $$;

-- Purchase bridge. The origin pins the policy snapshot, and with it the unit, when
-- the order is confirmed; every later receipt, bill and void of that order uses it.
CREATE OR REPLACE FUNCTION archava_accounting.guard_purchase_origin() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE b archava_accounting.books;p archava_accounting.purchase_policies;o archava_purchasing.orders;c archava_purchasing.commands;a archava_purchasing.audit;held jsonb;reviewed_at timestamptz;
BEGIN
 SELECT * INTO b FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 SELECT * INTO p FROM archava_accounting.purchase_policies WHERE tenant_id=NEW.tenant_id AND id=NEW.policy_id AND book_id=NEW.book_id;
 SELECT * INTO o FROM archava_purchasing.orders WHERE tenant_id=NEW.tenant_id AND id=NEW.order_id;
 SELECT * INTO c FROM archava_purchasing.commands WHERE tenant_id=NEW.tenant_id AND request_id=NEW.id;
 SELECT * INTO a FROM archava_purchasing.audit WHERE tenant_id=NEW.tenant_id AND request_id=NEW.id;
 SELECT snapshot->'record',occurred_at INTO held,reviewed_at FROM archava_accounting.audit WHERE tenant_id=NEW.tenant_id AND resource_kind='purchase_policy' AND resource_id=NEW.policy_id AND revision=NEW.policy_revision;
 IF b.id IS NULL OR b.source_authority<>'native' OR ((p.source_unit='whole_idr' AND b.currency='IDR' AND b.minor_units=2) OR (p.source_unit='minor' AND b.currency<>'IDR')) IS NOT TRUE
 OR o.id IS NULL OR o.currency IS DISTINCT FROM b.currency OR o.company_id IS DISTINCT FROM b.company_id OR o.confirmed_at IS NULL
 OR p.id IS NULL OR p.revision IS DISTINCT FROM NEW.policy_revision OR held IS NULL OR (held-'created_at') IS DISTINCT FROM (to_jsonb(p)-'created_at') OR (held->>'created_at')::timestamptz IS DISTINCT FROM p.created_at OR held IS DISTINCT FROM NEW.policy_snapshot
 OR a.request_id IS NULL OR a.operation<>'order.confirm' OR a.record_id IS DISTINCT FROM NEW.order_id OR a.revision IS DISTINCT FROM NEW.order_revision OR a.actor_id IS DISTINCT FROM NEW.actor_id OR a.occurred_at IS DISTINCT FROM NEW.occurred_at OR a.occurred_at<reviewed_at
 OR c.actor_id IS DISTINCT FROM NEW.actor_id OR c.receipt->>'operation' IS DISTINCT FROM 'order.confirm' OR c.receipt->>'id' IS DISTINCT FROM NEW.order_id::text OR c.receipt->>'revision' IS DISTINCT FROM NEW.order_revision::text THEN
  RAISE EXCEPTION 'Purchase origin differs from its confirmed order and reviewed current policy' USING ERRCODE='23514';
 END IF;
 RETURN NEW;
END $$;
-- Server-derived effect including the immutable operational receipt/version. The
-- unit is the one pinned by the order's origin. Under `whole_idr` the receipt
-- snapshot keeps its `unitCostWholeIdr` key; under `minor` the key is `unitCostMinor`.
CREATE OR REPLACE FUNCTION archava_accounting.purchase_effect(wanted_tenant text,wanted_request uuid) RETURNS jsonb LANGUAGE plpgsql VOLATILE SET search_path=pg_catalog AS $$
DECLARE c archava_purchasing.commands;a archava_purchasing.audit;r archava_purchasing.receipts;v archava_purchasing.supplier_document_versions;prior archava_purchasing.supplier_document_versions;goods numeric;fee numeric;stock numeric;quantity numeric;expected_quantity numeric;snapshot jsonb;effect text;po uuid;doc uuid;unit_basis text;unit_factor numeric;
BEGIN
 SELECT * INTO c FROM archava_purchasing.commands WHERE tenant_id=wanted_tenant AND request_id=wanted_request;
 SELECT * INTO a FROM archava_purchasing.audit WHERE tenant_id=wanted_tenant AND request_id=wanted_request;
 IF c.receipt IS NULL OR a.request_id IS NULL OR c.receipt->>'requestId' IS DISTINCT FROM wanted_request::text OR c.receipt->>'operation' IS DISTINCT FROM a.operation OR c.receipt->>'id' IS DISTINCT FROM a.record_id::text OR c.receipt->>'revision' IS DISTINCT FROM a.revision::text OR c.actor_id IS DISTINCT FROM a.actor_id THEN RETURN NULL;END IF;
 IF a.operation='order.receive' THEN
  SELECT * INTO r FROM archava_purchasing.receipts WHERE tenant_id=wanted_tenant AND request_id=wanted_request;
  IF r.request_id IS NULL OR r.order_id IS DISTINCT FROM a.record_id OR r.revision IS DISTINCT FROM a.revision OR r.actor_id IS DISTINCT FROM a.actor_id THEN RAISE EXCEPTION 'Missing immutable purchase receipt' USING ERRCODE='23514';END IF;
  po:=r.order_id;effect:='receipt';
  SELECT g.policy_snapshot->>'source_unit' INTO unit_basis FROM archava_accounting.purchase_origins g WHERE g.tenant_id=wanted_tenant AND g.order_id=po;
  SELECT coalesce(sum(l.unit_cost::numeric*x.accepted),0),coalesce(sum(x.accepted),0),jsonb_build_object('receipt',to_jsonb(r),'lines',coalesce(jsonb_agg(jsonb_build_object('receiptLine',to_jsonb(x),CASE WHEN unit_basis='minor' THEN 'unitCostMinor' ELSE 'unitCostWholeIdr' END,l.unit_cost) ORDER BY x.line_id),'[]'::jsonb))
  INTO goods,expected_quantity,snapshot FROM archava_purchasing.receipt_lines x JOIN archava_purchasing.lines l ON l.tenant_id=x.tenant_id AND l.id=x.line_id AND l.order_id=x.order_id WHERE x.tenant_id=wanted_tenant AND x.request_id=wanted_request;
  fee:=r.landed_fee_minor;
  SELECT coalesce(sum(total_cost_minor::numeric),0),coalesce(sum(original_quantity::numeric),0) INTO stock,quantity FROM archava_commerce.inventory_layers WHERE tenant_id=wanted_tenant AND source_request_id=wanted_request AND source_kind='purchase';
  IF stock IS DISTINCT FROM goods+fee OR quantity IS DISTINCT FROM expected_quantity OR EXISTS(SELECT 1 FROM archava_commerce.inventory_layers WHERE tenant_id=wanted_tenant AND source_request_id=wanted_request AND source_kind='purchase' AND total_cost_minor IS NULL) THEN RAISE EXCEPTION 'Purchase source differs from original accepted acquisition costs' USING ERRCODE='23514';END IF;
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
  SELECT g.policy_snapshot->>'source_unit' INTO unit_basis FROM archava_accounting.purchase_origins g WHERE g.tenant_id=wanted_tenant AND g.order_id=po;
  SELECT jsonb_build_object('version',to_jsonb(v),'priorMatchedVersion',CASE WHEN effect='void' THEN to_jsonb(prior) ELSE NULL END,'allocations',coalesce(jsonb_agg(to_jsonb(x) ORDER BY x.receipt_request_id,x.line_id),'[]'::jsonb)) INTO snapshot FROM archava_purchasing.supplier_document_allocations x WHERE x.tenant_id=wanted_tenant AND x.document_id=v.document_id AND x.document_revision=CASE WHEN effect='void' THEN prior.revision ELSE v.revision END;
 ELSE RETURN NULL;
 END IF;
 unit_factor:=CASE unit_basis WHEN 'whole_idr' THEN 100 WHEN 'minor' THEN 1 END;
 IF unit_factor IS NULL THEN RAISE EXCEPTION 'Purchase source has no origin with a reviewed source unit' USING ERRCODE='23514';END IF;
 IF goods IS NULL OR fee IS NULL OR abs(goods*unit_factor)>9000000000000000 OR fee*unit_factor>9000000000000000 OR (effect='receipt' AND (goods+fee)*unit_factor>9000000000000000) THEN RAISE EXCEPTION 'Purchase source minor-unit bound exceeded' USING ERRCODE='23514';END IF;
 RETURN jsonb_build_object('orderId',po,'documentId',doc,'revision',a.revision,'effect',effect,'goodsMinor',(goods*unit_factor)::bigint::text,'landedFeeMinor',(fee*unit_factor)::bigint::text,'actorId',a.actor_id,'occurredAt',a.occurred_at,'sourceDate',(a.occurred_at AT TIME ZONE 'UTC')::date,'snapshot',snapshot);
END $$;
-- Still quiet without a company, a book or a policy, and for an order confirmed
-- before its policy. With a policy, confirming an order in another currency than
-- the book is refused here, which refuses the confirmation.
CREATE OR REPLACE FUNCTION archava_accounting.capture_purchase_source() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE po uuid;company uuid;order_currency text;b archava_accounting.books;p archava_accounting.purchase_policies;o archava_accounting.purchase_origins;a archava_purchasing.audit;e jsonb;held jsonb;
BEGIN
 IF OLD.receipt IS NOT NULL OR NEW.receipt IS NULL THEN RETURN NEW;END IF;
 SELECT * INTO a FROM archava_purchasing.audit WHERE tenant_id=NEW.tenant_id AND request_id=NEW.request_id;
 IF a.operation='order.confirm' THEN
  SELECT company_id,currency INTO company,order_currency FROM archava_purchasing.orders WHERE tenant_id=NEW.tenant_id AND id=a.record_id;
  IF company IS NULL THEN RETURN NEW;END IF;
  SELECT * INTO b FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND company_id=company FOR UPDATE;
  IF b.id IS NULL THEN RETURN NEW;END IF;
  SELECT * INTO p FROM archava_accounting.purchase_policies WHERE tenant_id=NEW.tenant_id AND book_id=b.id;
  SELECT snapshot->'record' INTO held FROM archava_accounting.audit WHERE tenant_id=NEW.tenant_id AND resource_kind='purchase_policy' AND resource_id=p.id AND revision=p.revision;
  IF p.id IS NOT NULL THEN
   IF order_currency IS DISTINCT FROM b.currency THEN RAISE EXCEPTION 'Purchase order currency differs from its company book' USING ERRCODE='23514';END IF;
   INSERT INTO archava_accounting.purchase_origins(tenant_id,id,book_id,policy_id,policy_revision,policy_snapshot,order_id,order_revision,actor_id,occurred_at) VALUES(NEW.tenant_id,NEW.request_id,b.id,p.id,p.revision,held,a.record_id,a.revision,a.actor_id,a.occurred_at);
  END IF;
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

-- Stock bridge. Layer costs are in the sales order's currency; the unit is the
-- current policy's, which cannot change for a book. Still quiet without a policy.
-- With a policy, fulfilling or returning an order in another currency than the
-- book is refused, which refuses the fulfilment or return.
CREATE OR REPLACE FUNCTION archava_accounting.stock_effect(wanted_tenant text,wanted_request uuid,wanted_book uuid) RETURNS jsonb LANGUAGE plpgsql VOLATILE SET search_path=pg_catalog AS $$
DECLARE c archava_commerce.commands;a archava_commerce.audit;o archava_commerce.orders;b archava_accounting.books;p archava_accounting.stock_policies;
 part record;origin archava_accounting.stock_layer_origins;parent archava_accounting.stock_sources;old_segment jsonb;policy jsonb;segments jsonb:='[]';snapshot jsonb:='[]';total numeric:=0;cost numeric;effect text;tracked boolean;wanted_cogs uuid;sign integer;unit_factor numeric;
BEGIN
 SELECT * INTO c FROM archava_commerce.commands WHERE tenant_id=wanted_tenant AND request_id=wanted_request;
 SELECT * INTO a FROM archava_commerce.audit WHERE tenant_id=wanted_tenant AND request_id=wanted_request;
 IF c.receipt IS NULL OR a.request_id IS NULL OR c.receipt->>'kind' IS DISTINCT FROM 'order' OR c.receipt->>'id' IS DISTINCT FROM a.record_id::text OR c.receipt->>'revision' IS DISTINCT FROM a.revision::text OR c.actor_id IS DISTINCT FROM a.actor_id OR a.operation NOT IN('order.fulfill','order.return') THEN RETURN NULL;END IF;
 SELECT * INTO o FROM archava_commerce.orders WHERE tenant_id=wanted_tenant AND id=a.record_id;
 SELECT * INTO b FROM archava_accounting.books WHERE tenant_id=wanted_tenant AND id=wanted_book;
 SELECT * INTO p FROM archava_accounting.stock_policies WHERE tenant_id=wanted_tenant AND book_id=wanted_book;
 IF p.id IS NULL THEN RETURN NULL;END IF;
 unit_factor:=CASE WHEN p.source_unit='whole_idr' AND b.currency='IDR' AND b.minor_units=2 THEN 100 WHEN p.source_unit='minor' AND b.currency<>'IDR' THEN 1 END;
 IF b.id IS NULL OR b.company_id IS DISTINCT FROM o.company_id OR b.source_authority<>'native' OR unit_factor IS NULL OR o.revision IS DISTINCT FROM a.revision THEN RAISE EXCEPTION 'Stock source differs from its current operating book/order' USING ERRCODE='23514';END IF;
 IF o.currency IS DISTINCT FROM b.currency THEN RAISE EXCEPTION 'Sales order currency differs from its company book' USING ERRCODE='23514';END IF;
 SELECT policy_audit.snapshot->'record' INTO policy FROM archava_accounting.audit policy_audit WHERE policy_audit.tenant_id=wanted_tenant AND policy_audit.resource_kind='stock_policy' AND policy_audit.resource_id=p.id AND policy_audit.revision=p.revision;
 IF policy IS NULL OR (policy-'created_at') IS DISTINCT FROM (to_jsonb(p)-'created_at') OR (policy->>'created_at')::timestamptz IS DISTINCT FROM p.created_at THEN RAISE EXCEPTION 'Stock source has no reviewed policy snapshot' USING ERRCODE='23514';END IF;
 IF a.operation='order.fulfill' THEN
  effect:='fulfillment';sign:=1;
  SELECT EXISTS(SELECT 1 FROM archava_commerce.order_lines WHERE tenant_id=wanted_tenant AND order_id=o.id AND track_stock) INTO tracked;
  IF tracked AND NOT EXISTS(SELECT 1 FROM archava_commerce.inventory_uses WHERE tenant_id=wanted_tenant AND request_id=wanted_request) THEN RAISE EXCEPTION 'Tracked stock requires actual original cost portions' USING ERRCODE='23514';END IF;
  FOR part IN SELECT u.*,to_jsonb(u)-'returned_quantity' AS evidence FROM archava_commerce.inventory_uses u WHERE tenant_id=wanted_tenant AND request_id=wanted_request AND order_id=o.id ORDER BY id LOOP
   SELECT * INTO origin FROM archava_accounting.stock_layer_origins WHERE tenant_id=wanted_tenant AND layer_id=part.layer_id;
   IF origin.layer_id IS NULL OR origin.book_id<>wanted_book OR part.total_cost_minor IS NULL THEN RAISE EXCEPTION 'Stock needs its reviewed original book/account/cost origin' USING ERRCODE='23514';END IF;
   cost:=part.total_cost_minor::numeric*unit_factor;total:=total+cost;
   IF cost>9000000000000000 OR total>9000000000000000 THEN RAISE EXCEPTION 'Stock source amount bound exceeded' USING ERRCODE='23514';END IF;
   segments:=segments||jsonb_build_array(jsonb_build_object('sourceUseId',part.id::text,'layerId',part.layer_id::text,'inventoryAccountId',origin.inventory_account_id,'cogsAccountId',p.cogs_account_id,'costMinor',cost::bigint::text,'originalStockSourceId',NULL));snapshot:=snapshot||jsonb_build_array(part.evidence);
  END LOOP;
 ELSE
  SELECT CASE WHEN disposition='restock' THEN 'restock_return' ELSE 'discard_return' END INTO effect FROM archava_commerce.returns WHERE tenant_id=wanted_tenant AND request_id=wanted_request AND order_id=o.id;
  IF effect IS NULL THEN RAISE EXCEPTION 'Stock return needs its immutable physical disposition' USING ERRCODE='23514';END IF;sign:=-1;
  FOR part IN SELECT r.*,u.layer_id,u.request_id AS use_request,to_jsonb(r) AS evidence FROM archava_commerce.inventory_return_costs r LEFT JOIN archava_commerce.inventory_uses u ON u.tenant_id=r.tenant_id AND u.id=r.source_use_id WHERE r.tenant_id=wanted_tenant AND r.request_id=wanted_request ORDER BY r.id LOOP
   snapshot:=snapshot||jsonb_build_array(part.evidence);
   IF NOT part.restock THEN CONTINUE;END IF;
   SELECT * INTO origin FROM archava_accounting.stock_layer_origins WHERE tenant_id=wanted_tenant AND layer_id=part.layer_id;
   SELECT prior_source.* INTO parent FROM archava_accounting.stock_sources prior_source WHERE prior_source.tenant_id=wanted_tenant AND prior_source.book_id=wanted_book AND prior_source.id=part.use_request AND prior_source.effect='fulfillment';
   SELECT v INTO old_segment FROM jsonb_array_elements(parent.segments) v WHERE v->>'sourceUseId'=part.source_use_id::text;
   IF origin.layer_id IS NULL OR origin.book_id<>wanted_book OR part.total_cost_minor IS NULL OR parent.id IS NULL OR old_segment IS NULL OR old_segment->>'inventoryAccountId' IS DISTINCT FROM origin.inventory_account_id::text THEN RAISE EXCEPTION 'Restock requires its original recognized fulfillment COGS source' USING ERRCODE='23514';END IF;
   wanted_cogs:=(old_segment->>'cogsAccountId')::uuid;cost:=part.total_cost_minor::numeric*unit_factor;total:=total+cost;
   IF cost>9000000000000000 OR total>9000000000000000 OR cost>(old_segment->>'costMinor')::numeric THEN RAISE EXCEPTION 'Returned stock exceeds its original cost source bound' USING ERRCODE='23514';END IF;
   segments:=segments||jsonb_build_array(jsonb_build_object('sourceUseId',part.source_use_id::text,'returnCostId',part.id::text,'layerId',part.layer_id::text,'inventoryAccountId',origin.inventory_account_id,'cogsAccountId',wanted_cogs,'costMinor',cost::bigint::text,'originalStockSourceId',parent.id));
  END LOOP;
 END IF;
 IF jsonb_array_length(segments)>10000 OR total>9000000000000000 THEN RAISE EXCEPTION 'Stock source bound exceeded' USING ERRCODE='23514';END IF;
 IF (SELECT count(DISTINCT account) FROM(SELECT v->>'inventoryAccountId' AS account FROM jsonb_array_elements(segments) v WHERE (v->>'costMinor')::numeric<>0 UNION ALL SELECT v->>'cogsAccountId' FROM jsonb_array_elements(segments) v WHERE (v->>'costMinor')::numeric<>0) effective_accounts)>100 THEN RAISE EXCEPTION 'Split stock operations before exceeding the supported journal account bound' USING ERRCODE='23514';END IF;
 RETURN jsonb_build_object('orderId',o.id,'sourceRevision',a.revision,'effect',effect,'amountMinor',(total*sign)::bigint::text,'policyId',p.id,'policyRevision',p.revision,'policySnapshot',policy,'segments',segments,'snapshot',snapshot,'actorId',a.actor_id,'occurredAt',a.occurred_at,'sourceDate',(a.occurred_at AT TIME ZONE 'UTC')::date);
END $$;
