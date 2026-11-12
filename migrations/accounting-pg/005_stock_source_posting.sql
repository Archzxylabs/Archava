-- Exact future stock-cost sources. No default books/policies or money backfill.
DO $$ BEGIN
 PERFORM pg_advisory_xact_lock(hashtext('archava-serving-writer-takeover'));
 IF to_regclass('archava_control.writer_lease') IS NOT NULL THEN
  PERFORM 1 FROM archava_control.writer_lease WHERE lease_name='archava-serving' FOR UPDATE;
 END IF;
END $$;
-- Attach only exact original receipt/return lineage; never infer an opening balance.
CREATE TABLE archava_accounting.stock_layer_origins(
 tenant_id text NOT NULL,layer_id bigint NOT NULL,book_id uuid NOT NULL,inventory_account_id uuid NOT NULL,
 purchase_source_id uuid NOT NULL,parent_layer_id bigint,
 PRIMARY KEY(tenant_id,layer_id),
 FOREIGN KEY(tenant_id,layer_id) REFERENCES archava_commerce.inventory_layers(tenant_id,id),
 FOREIGN KEY(tenant_id,book_id,inventory_account_id) REFERENCES archava_accounting.accounts(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,book_id,purchase_source_id) REFERENCES archava_accounting.purchase_sources(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,parent_layer_id) REFERENCES archava_accounting.stock_layer_origins(tenant_id,layer_id)
);
ALTER TABLE archava_accounting.stock_layer_origins ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_accounting.stock_layer_origins FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_scope ON archava_accounting.stock_layer_origins USING(tenant_id=current_setting('archava.tenant_id',true)) WITH CHECK(tenant_id=current_setting('archava.tenant_id',true));
CREATE TRIGGER stock_layer_origin_immutable BEFORE UPDATE OR DELETE ON archava_accounting.stock_layer_origins FOR EACH ROW EXECUTE FUNCTION archava_accounting.immutable();
CREATE FUNCTION archava_accounting.guard_stock_layer_origin() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE l archava_commerce.inventory_layers;s archava_accounting.purchase_sources;o archava_accounting.purchase_origins;p archava_accounting.stock_layer_origins;parent bigint;
BEGIN
 SELECT * INTO l FROM archava_commerce.inventory_layers WHERE tenant_id=NEW.tenant_id AND id=NEW.layer_id;
 SELECT * INTO s FROM archava_accounting.purchase_sources WHERE tenant_id=NEW.tenant_id AND id=NEW.purchase_source_id AND book_id=NEW.book_id AND effect='receipt';
 SELECT * INTO o FROM archava_accounting.purchase_origins WHERE tenant_id=NEW.tenant_id AND id=s.origin_id;
 IF s.id IS NULL OR o.id IS NULL OR NOT EXISTS(SELECT 1 FROM jsonb_array_elements(o.policy_snapshot->'mappings') m WHERE m->>'role'='inventory' AND m->>'accountId'=NEW.inventory_account_id::text) THEN RAISE EXCEPTION 'Layer requires its original captured receipt account' USING ERRCODE='23514';END IF;
 IF l.source_kind='purchase' THEN
  IF l.source_request_id IS DISTINCT FROM s.id OR NEW.parent_layer_id IS NOT NULL THEN RAISE EXCEPTION 'Layer is not from the captured receipt' USING ERRCODE='23514';END IF;
 ELSIF l.source_kind='return' THEN
  SELECT u.layer_id INTO parent FROM archava_commerce.inventory_return_costs r JOIN archava_commerce.inventory_uses u ON u.tenant_id=r.tenant_id AND u.id=r.source_use_id WHERE r.tenant_id=l.tenant_id AND r.id=l.return_cost_id AND r.request_id=l.source_request_id AND r.restock;
  SELECT * INTO p FROM archava_accounting.stock_layer_origins WHERE tenant_id=l.tenant_id AND layer_id=parent;
  IF p.layer_id IS NULL OR (NEW.book_id,NEW.inventory_account_id,NEW.purchase_source_id,NEW.parent_layer_id) IS DISTINCT FROM (p.book_id,p.inventory_account_id,p.purchase_source_id,p.layer_id) THEN RAISE EXCEPTION 'Returned layer has no exact original account provenance' USING ERRCODE='23514';END IF;
 ELSE RAISE EXCEPTION 'Layer requires an explicit reviewed opening origin' USING ERRCODE='23514';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER stock_layer_origin_guard BEFORE INSERT ON archava_accounting.stock_layer_origins FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_stock_layer_origin();
CREATE FUNCTION archava_accounting.bind_purchase_layers() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog AS $$
BEGIN
 IF NEW.effect='receipt' THEN
  INSERT INTO archava_accounting.stock_layer_origins(tenant_id,layer_id,book_id,inventory_account_id,purchase_source_id)
  SELECT l.tenant_id,l.id,NEW.book_id,(m->>'accountId')::uuid,NEW.id FROM archava_commerce.inventory_layers l JOIN archava_accounting.purchase_origins o ON o.tenant_id=NEW.tenant_id AND o.id=NEW.origin_id CROSS JOIN LATERAL jsonb_array_elements(o.policy_snapshot->'mappings') m
  WHERE l.tenant_id=NEW.tenant_id AND l.source_request_id=NEW.id AND l.source_kind='purchase' AND m->>'role'='inventory';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER bind_purchase_layers AFTER INSERT ON archava_accounting.purchase_sources FOR EACH ROW EXECUTE FUNCTION archava_accounting.bind_purchase_layers();
CREATE FUNCTION archava_accounting.bind_returned_layer() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog AS $$
BEGIN
 IF NEW.source_kind='return' AND NEW.return_cost_id IS NOT NULL THEN
  INSERT INTO archava_accounting.stock_layer_origins(tenant_id,layer_id,book_id,inventory_account_id,purchase_source_id,parent_layer_id)
  SELECT NEW.tenant_id,NEW.id,p.book_id,p.inventory_account_id,p.purchase_source_id,p.layer_id FROM archava_commerce.inventory_return_costs r JOIN archava_commerce.inventory_uses u ON u.tenant_id=r.tenant_id AND u.id=r.source_use_id JOIN archava_accounting.stock_layer_origins p ON p.tenant_id=u.tenant_id AND p.layer_id=u.layer_id WHERE r.tenant_id=NEW.tenant_id AND r.id=NEW.return_cost_id;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER bind_returned_layer AFTER INSERT ON archava_commerce.inventory_layers FOR EACH ROW EXECUTE FUNCTION archava_accounting.bind_returned_layer();

REVOKE ALL ON FUNCTION archava_accounting.bind_purchase_layers(),archava_accounting.bind_returned_layer() FROM PUBLIC;

-- Captured original stock costs are reviewed into separate ordinary journal drafts.
ALTER TABLE archava_accounting.audit DROP CONSTRAINT audit_resource_kind_check;
ALTER TABLE archava_accounting.audit ADD CONSTRAINT audit_resource_kind_check CHECK(resource_kind IN('book','account','period','journal','invoice_policy','purchase_policy','stock_policy'));
CREATE TABLE archava_accounting.stock_policies(
 tenant_id text NOT NULL,id uuid NOT NULL,book_id uuid NOT NULL,currency_catalogue text NOT NULL CHECK(currency_catalogue='SIX-2026-09-17'),source_unit text NOT NULL CHECK(source_unit='whole_idr'),
 cogs_account_id uuid NOT NULL,account_revision integer NOT NULL CHECK(account_revision>0),revision integer NOT NULL DEFAULT 1 CHECK(revision>0),created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,book_id),UNIQUE(tenant_id,book_id,id),FOREIGN KEY(tenant_id,book_id,cogs_account_id) REFERENCES archava_accounting.accounts(tenant_id,book_id,id)
);
CREATE TABLE archava_accounting.stock_sources(
 tenant_id text NOT NULL,id uuid NOT NULL,book_id uuid NOT NULL,order_id uuid NOT NULL,source_revision integer NOT NULL,
 effect text NOT NULL CHECK(effect IN('fulfillment','restock_return','discard_return')),amount_minor bigint NOT NULL CHECK(abs(amount_minor::numeric)<=9000000000000000),
 policy_id uuid NOT NULL,policy_revision integer NOT NULL CHECK(policy_revision>0),policy_snapshot jsonb NOT NULL CHECK(jsonb_typeof(policy_snapshot)='object'),segments jsonb NOT NULL CHECK(jsonb_typeof(segments)='array' AND jsonb_array_length(segments)<=10000),source_snapshot jsonb NOT NULL CHECK(jsonb_typeof(source_snapshot)='array'),
 actor_id text NOT NULL,occurred_at timestamptz NOT NULL CHECK(isfinite(occurred_at)),source_date date NOT NULL CHECK(isfinite(source_date)),
 CHECK(source_revision>0),CHECK((effect='fulfillment' AND amount_minor>=0) OR (effect='restock_return' AND amount_minor<=0) OR (effect='discard_return' AND amount_minor=0)),
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,book_id,id),UNIQUE(tenant_id,order_id,source_revision),FOREIGN KEY(tenant_id,book_id) REFERENCES archava_accounting.books(tenant_id,id),
 FOREIGN KEY(tenant_id,order_id) REFERENCES archava_commerce.orders(tenant_id,id),FOREIGN KEY(tenant_id,id) REFERENCES archava_commerce.commands(tenant_id,request_id),
 FOREIGN KEY(tenant_id,book_id,policy_id) REFERENCES archava_accounting.stock_policies(tenant_id,book_id,id)
);
CREATE INDEX stock_source_book ON archava_accounting.stock_sources(tenant_id,book_id,source_date,id);
CREATE TABLE archava_accounting.stock_journal_sources(
 tenant_id text NOT NULL,book_id uuid NOT NULL,journal_id uuid NOT NULL,source_id uuid NOT NULL,
 PRIMARY KEY(tenant_id,journal_id),FOREIGN KEY(tenant_id,book_id,journal_id) REFERENCES archava_accounting.journals(tenant_id,book_id,id),FOREIGN KEY(tenant_id,book_id,source_id) REFERENCES archava_accounting.stock_sources(tenant_id,book_id,id)
);
DO $$ DECLARE owned text;BEGIN FOREACH owned IN ARRAY ARRAY['stock_policies','stock_sources','stock_journal_sources'] LOOP
 EXECUTE format('ALTER TABLE archava_accounting.%I ENABLE ROW LEVEL SECURITY',owned);
 EXECUTE format('ALTER TABLE archava_accounting.%I FORCE ROW LEVEL SECURITY',owned);
 EXECUTE format('CREATE POLICY accounting_tenant ON archava_accounting.%I USING(tenant_id=current_setting(''archava.tenant_id'',true)) WITH CHECK(tenant_id=current_setting(''archava.tenant_id'',true))',owned);
END LOOP;END $$;
CREATE TRIGGER stock_source_immutable BEFORE UPDATE OR DELETE ON archava_accounting.stock_sources FOR EACH ROW EXECUTE FUNCTION archava_accounting.immutable();
CREATE TRIGGER stock_journal_source_immutable BEFORE UPDATE OR DELETE ON archava_accounting.stock_journal_sources FOR EACH ROW EXECUTE FUNCTION archava_accounting.immutable();

CREATE FUNCTION archava_accounting.prepare_stock_source() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF NEW.company_id IS NOT NULL THEN
  PERFORM archava_accounting.lock_purchase_company(NEW.tenant_id,NEW.company_id);
  PERFORM 1 FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND company_id=NEW.company_id FOR UPDATE;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER aa_stock_source_boundary BEFORE UPDATE ON archava_commerce.orders FOR EACH ROW EXECUTE FUNCTION archava_accounting.prepare_stock_source();

CREATE FUNCTION archava_accounting.guard_stock_policy() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE b archava_accounting.books;a archava_accounting.accounts;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Stock posting policy history cannot be deleted' USING ERRCODE='23514';END IF;
 SELECT * INTO b FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 SELECT * INTO a FROM archava_accounting.accounts WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND id=NEW.cogs_account_id FOR SHARE;
 IF b.id IS NULL OR b.source_authority<>'native' OR b.currency<>'IDR' OR b.minor_units<>2 OR b.currency_catalogue<>NEW.currency_catalogue OR a.id IS NULL OR NOT a.active OR a.revision<>NEW.account_revision OR a.class<>'cost_of_goods_sold' OR a.cash_role<>'operating' THEN RAISE EXCEPTION 'Stock policy requires a reviewed native IDR book and current COGS account' USING ERRCODE='23514';END IF;
 IF TG_OP='INSERT' THEN
  IF NEW.revision<>1 THEN RAISE EXCEPTION 'Initial stock policy revision refused' USING ERRCODE='23514';END IF;
 ELSIF (to_jsonb(NEW)-ARRAY['revision','cogs_account_id','account_revision']) IS DISTINCT FROM (to_jsonb(OLD)-ARRAY['revision','cogs_account_id','account_revision']) OR NEW.revision<>OLD.revision+1 THEN RAISE EXCEPTION 'Stock policy identity and unit basis are immutable' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER stock_policy_guard BEFORE INSERT OR UPDATE OR DELETE ON archava_accounting.stock_policies FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_stock_policy();
CREATE FUNCTION archava_accounting.assert_stock_policy() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE a archava_accounting.audit;c archava_accounting.commands;
BEGIN
 SELECT * INTO a FROM archava_accounting.audit WHERE tenant_id=NEW.tenant_id AND resource_kind='stock_policy' AND resource_id=NEW.id AND revision=NEW.revision;
 SELECT * INTO c FROM archava_accounting.commands WHERE tenant_id=NEW.tenant_id AND request_id=a.request_id;
 IF a.operation IS DISTINCT FROM 'stock_policy.set' OR a.book_id IS DISTINCT FROM NEW.book_id OR a.snapshot->'record' IS DISTINCT FROM to_jsonb(NEW)
 OR c.actor_id IS DISTINCT FROM a.actor_id OR c.receipt->>'resourceKind' IS DISTINCT FROM 'stock_policy' OR c.receipt->>'resourceId' IS DISTINCT FROM NEW.id::text OR c.receipt->>'revision' IS DISTINCT FROM NEW.revision::text OR c.receipt->>'type' IS DISTINCT FROM 'stock_policy.set' THEN RAISE EXCEPTION 'Stock policy requires its immutable reviewed command and audit' USING ERRCODE='23514';END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER stock_policy_complete AFTER INSERT OR UPDATE ON archava_accounting.stock_policies DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_accounting.assert_stock_policy();

CREATE FUNCTION archava_accounting.stock_effect(wanted_tenant text,wanted_request uuid,wanted_book uuid) RETURNS jsonb LANGUAGE plpgsql VOLATILE SET search_path=pg_catalog AS $$
DECLARE c archava_commerce.commands;a archava_commerce.audit;o archava_commerce.orders;b archava_accounting.books;p archava_accounting.stock_policies;
 part record;origin archava_accounting.stock_layer_origins;parent archava_accounting.stock_sources;old_segment jsonb;policy jsonb;segments jsonb:='[]';snapshot jsonb:='[]';total numeric:=0;cost numeric;effect text;tracked boolean;wanted_cogs uuid;sign integer;
BEGIN
 SELECT * INTO c FROM archava_commerce.commands WHERE tenant_id=wanted_tenant AND request_id=wanted_request;
 SELECT * INTO a FROM archava_commerce.audit WHERE tenant_id=wanted_tenant AND request_id=wanted_request;
 IF c.receipt IS NULL OR a.request_id IS NULL OR c.receipt->>'kind' IS DISTINCT FROM 'order' OR c.receipt->>'id' IS DISTINCT FROM a.record_id::text OR c.receipt->>'revision' IS DISTINCT FROM a.revision::text OR c.actor_id IS DISTINCT FROM a.actor_id OR a.operation NOT IN('order.fulfill','order.return') THEN RETURN NULL;END IF;
 SELECT * INTO o FROM archava_commerce.orders WHERE tenant_id=wanted_tenant AND id=a.record_id;
 SELECT * INTO b FROM archava_accounting.books WHERE tenant_id=wanted_tenant AND id=wanted_book;
 SELECT * INTO p FROM archava_accounting.stock_policies WHERE tenant_id=wanted_tenant AND book_id=wanted_book;
 IF p.id IS NULL THEN RETURN NULL;END IF;
 IF b.id IS NULL OR b.company_id IS DISTINCT FROM o.company_id OR b.source_authority<>'native' OR b.currency<>'IDR' OR b.minor_units<>2 OR o.revision IS DISTINCT FROM a.revision THEN RAISE EXCEPTION 'Stock source differs from its current operating book/order' USING ERRCODE='23514';END IF;
 SELECT policy_audit.snapshot->'record' INTO policy FROM archava_accounting.audit policy_audit WHERE policy_audit.tenant_id=wanted_tenant AND policy_audit.resource_kind='stock_policy' AND policy_audit.resource_id=p.id AND policy_audit.revision=p.revision;
 IF policy IS NULL OR (policy-'created_at') IS DISTINCT FROM (to_jsonb(p)-'created_at') OR (policy->>'created_at')::timestamptz IS DISTINCT FROM p.created_at THEN RAISE EXCEPTION 'Stock source has no reviewed policy snapshot' USING ERRCODE='23514';END IF;
 IF a.operation='order.fulfill' THEN
  effect:='fulfillment';sign:=1;
  SELECT EXISTS(SELECT 1 FROM archava_commerce.order_lines WHERE tenant_id=wanted_tenant AND order_id=o.id AND track_stock) INTO tracked;
  IF tracked AND NOT EXISTS(SELECT 1 FROM archava_commerce.inventory_uses WHERE tenant_id=wanted_tenant AND request_id=wanted_request) THEN RAISE EXCEPTION 'Tracked stock requires actual original cost portions' USING ERRCODE='23514';END IF;
  FOR part IN SELECT u.*,to_jsonb(u)-'returned_quantity' AS evidence FROM archava_commerce.inventory_uses u WHERE tenant_id=wanted_tenant AND request_id=wanted_request AND order_id=o.id ORDER BY id LOOP
   SELECT * INTO origin FROM archava_accounting.stock_layer_origins WHERE tenant_id=wanted_tenant AND layer_id=part.layer_id;
   IF origin.layer_id IS NULL OR origin.book_id<>wanted_book OR part.total_cost_minor IS NULL THEN RAISE EXCEPTION 'Stock needs its reviewed original book/account/cost origin' USING ERRCODE='23514';END IF;
   cost:=part.total_cost_minor::numeric*100;total:=total+cost;
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
   wanted_cogs:=(old_segment->>'cogsAccountId')::uuid;cost:=part.total_cost_minor::numeric*100;total:=total+cost;
   IF cost>9000000000000000 OR total>9000000000000000 OR cost>(old_segment->>'costMinor')::numeric THEN RAISE EXCEPTION 'Returned stock exceeds its original cost source bound' USING ERRCODE='23514';END IF;
   segments:=segments||jsonb_build_array(jsonb_build_object('sourceUseId',part.source_use_id::text,'returnCostId',part.id::text,'layerId',part.layer_id::text,'inventoryAccountId',origin.inventory_account_id,'cogsAccountId',wanted_cogs,'costMinor',cost::bigint::text,'originalStockSourceId',parent.id));
  END LOOP;
 END IF;
 IF jsonb_array_length(segments)>10000 OR total>9000000000000000 THEN RAISE EXCEPTION 'Stock source bound exceeded' USING ERRCODE='23514';END IF;
 IF (SELECT count(DISTINCT account) FROM(SELECT v->>'inventoryAccountId' AS account FROM jsonb_array_elements(segments) v WHERE (v->>'costMinor')::numeric<>0 UNION ALL SELECT v->>'cogsAccountId' FROM jsonb_array_elements(segments) v WHERE (v->>'costMinor')::numeric<>0) effective_accounts)>100 THEN RAISE EXCEPTION 'Split stock operations before exceeding the supported journal account bound' USING ERRCODE='23514';END IF;
 RETURN jsonb_build_object('orderId',o.id,'sourceRevision',a.revision,'effect',effect,'amountMinor',(total*sign)::bigint::text,'policyId',p.id,'policyRevision',p.revision,'policySnapshot',policy,'segments',segments,'snapshot',snapshot,'actorId',a.actor_id,'occurredAt',a.occurred_at,'sourceDate',(a.occurred_at AT TIME ZONE 'UTC')::date);
END $$;
CREATE FUNCTION archava_accounting.guard_stock_source() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE expected jsonb;
BEGIN
 PERFORM 1 FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 expected:=archava_accounting.stock_effect(NEW.tenant_id,NEW.id,NEW.book_id);
 IF expected IS NULL OR expected IS DISTINCT FROM jsonb_build_object('orderId',NEW.order_id,'sourceRevision',NEW.source_revision,'effect',NEW.effect,'amountMinor',NEW.amount_minor::text,'policyId',NEW.policy_id,'policyRevision',NEW.policy_revision,'policySnapshot',NEW.policy_snapshot,'segments',NEW.segments,'snapshot',NEW.source_snapshot,'actorId',NEW.actor_id,'occurredAt',NEW.occurred_at,'sourceDate',NEW.source_date) THEN RAISE EXCEPTION 'Stock source differs from its actual immutable cost effect' USING ERRCODE='23514';END IF;
 IF NEW.amount_minor<>0 AND EXISTS(SELECT 1 FROM archava_accounting.periods WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND NEW.source_date BETWEEN start_date AND end_date AND state='closed') THEN RAISE EXCEPTION 'Stock source falls in a closed financial period' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER stock_source_guard BEFORE INSERT ON archava_accounting.stock_sources FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_stock_source();
CREATE FUNCTION archava_accounting.capture_stock_source() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE company uuid;book uuid;e jsonb;
BEGIN
 IF OLD.receipt IS NOT NULL OR NEW.receipt IS NULL THEN RETURN NEW;END IF;
 SELECT o.company_id INTO company FROM archava_commerce.orders o JOIN archava_commerce.audit a ON a.tenant_id=o.tenant_id AND a.record_id=o.id WHERE a.tenant_id=NEW.tenant_id AND a.request_id=NEW.request_id AND a.operation IN('order.fulfill','order.return');
 IF company IS NULL THEN RETURN NEW;END IF;
 PERFORM archava_accounting.lock_purchase_company(NEW.tenant_id,company);
 SELECT id INTO book FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND company_id=company FOR UPDATE;
 IF book IS NULL THEN RETURN NEW;END IF;e:=archava_accounting.stock_effect(NEW.tenant_id,NEW.request_id,book);IF e IS NULL THEN RETURN NEW;END IF;
 INSERT INTO archava_accounting.stock_sources(tenant_id,id,book_id,order_id,source_revision,effect,amount_minor,policy_id,policy_revision,policy_snapshot,segments,source_snapshot,actor_id,occurred_at,source_date)
 VALUES(NEW.tenant_id,NEW.request_id,book,(e->>'orderId')::uuid,(e->>'sourceRevision')::integer,e->>'effect',(e->>'amountMinor')::bigint,(e->>'policyId')::uuid,(e->>'policyRevision')::integer,e->'policySnapshot',e->'segments',e->'snapshot',e->>'actorId',(e->>'occurredAt')::timestamptz,(e->>'sourceDate')::date);
 RETURN NEW;
END $$;
CREATE TRIGGER capture_financial_stock_source AFTER UPDATE OF receipt ON archava_commerce.commands FOR EACH ROW EXECUTE FUNCTION archava_accounting.capture_stock_source();

CREATE FUNCTION archava_accounting.stock_legs(wanted_tenant text,wanted_source uuid) RETURNS TABLE(account_id uuid,debit_minor bigint,credit_minor bigint) LANGUAGE sql STABLE SET search_path=pg_catalog AS $$
 WITH s AS(SELECT * FROM archava_accounting.stock_sources WHERE tenant_id=wanted_tenant AND id=wanted_source),raw AS(
 SELECT (v->>'cogsAccountId')::uuid AS account_id,CASE WHEN s.effect='fulfillment' THEN (v->>'costMinor')::numeric ELSE -(v->>'costMinor')::numeric END AS amount FROM s CROSS JOIN LATERAL jsonb_array_elements(s.segments) v
 UNION ALL SELECT (v->>'inventoryAccountId')::uuid,CASE WHEN s.effect='fulfillment' THEN -(v->>'costMinor')::numeric ELSE (v->>'costMinor')::numeric END FROM s CROSS JOIN LATERAL jsonb_array_elements(s.segments) v
 ),totals AS(SELECT account_id,sum(amount) AS amount FROM raw GROUP BY account_id)
 SELECT account_id,greatest(amount,0)::bigint,greatest(-amount,0)::bigint FROM totals WHERE amount<>0 ORDER BY account_id
$$;

CREATE FUNCTION archava_accounting.guard_stock_journal_source() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE s archava_accounting.stock_sources;j archava_accounting.journals;expected_count integer;
BEGIN
 PERFORM 1 FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 SELECT * INTO s FROM archava_accounting.stock_sources WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND id=NEW.source_id;
 SELECT * INTO j FROM archava_accounting.journals WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND id=NEW.journal_id;
 SELECT count(*) INTO expected_count FROM archava_accounting.stock_legs(NEW.tenant_id,NEW.source_id);
 IF s.id IS NULL OR expected_count NOT BETWEEN 2 AND 100 OR j.id IS NULL OR j.state<>'draft' OR j.reversal_of IS NOT NULL OR j.entry_date<>s.source_date OR j.source_ref<>'stock-cost:'||s.id::text||':'||j.code
 OR EXISTS(SELECT 1 FROM archava_accounting.stock_journal_sources x JOIN archava_accounting.journals y ON y.tenant_id=x.tenant_id AND y.id=x.journal_id WHERE x.tenant_id=NEW.tenant_id AND x.source_id=NEW.source_id AND y.state<>'void') THEN RAISE EXCEPTION 'A stock source requires one reviewed live draft' USING ERRCODE='23514';END IF;
 IF (SELECT count(*) FROM archava_accounting.journal_lines WHERE tenant_id=NEW.tenant_id AND journal_id=j.id)<>expected_count OR EXISTS(
 SELECT 1 FROM archava_accounting.stock_legs(NEW.tenant_id,NEW.source_id) l WHERE NOT EXISTS(SELECT 1 FROM archava_accounting.journal_lines x WHERE x.tenant_id=NEW.tenant_id AND x.journal_id=j.id AND x.account_id=l.account_id AND x.debit_minor=l.debit_minor AND x.credit_minor=l.credit_minor)) THEN RAISE EXCEPTION 'Stock draft differs from its original costs/accounts' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER stock_journal_source_guard BEFORE INSERT ON archava_accounting.stock_journal_sources FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_stock_journal_source();
CREATE FUNCTION archava_accounting.assert_stock_journal_claim() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE wanted uuid;j archava_accounting.journals;s uuid;op text;r jsonb;
BEGIN
 wanted:=coalesce((to_jsonb(NEW)->>'journal_id')::uuid,(to_jsonb(NEW)->>'id')::uuid);
 SELECT * INTO j FROM archava_accounting.journals WHERE tenant_id=NEW.tenant_id AND id=wanted;
 SELECT source_id INTO s FROM archava_accounting.stock_journal_sources WHERE tenant_id=NEW.tenant_id AND journal_id=wanted;
 SELECT a.operation,c.receipt INTO op,r FROM archava_accounting.audit a JOIN archava_accounting.commands c ON c.tenant_id=a.tenant_id AND c.request_id=a.request_id WHERE a.tenant_id=NEW.tenant_id AND a.resource_kind='journal' AND a.resource_id=wanted AND a.revision=1;
 IF s IS NOT NULL THEN
  IF op IS DISTINCT FROM 'stock_source.draft' OR r->>'sourceId' IS DISTINCT FROM s::text THEN RAISE EXCEPTION 'Stock source draft must retain its reviewed command receipt' USING ERRCODE='23514';END IF;
 ELSIF op='stock_source.draft' OR j.source_ref LIKE 'stock-cost:%' THEN RAISE EXCEPTION 'Reserved stock source references require a bound stock source' USING ERRCODE='23514';END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER stock_journal_claim_complete AFTER INSERT ON archava_accounting.journals DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_accounting.assert_stock_journal_claim();
CREATE CONSTRAINT TRIGGER stock_link_claim_complete AFTER INSERT ON archava_accounting.stock_journal_sources DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_accounting.assert_stock_journal_claim();
CREATE FUNCTION archava_accounting.stock_close_guard() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF NEW.state='closed' AND EXISTS(SELECT 1 FROM archava_accounting.stock_sources s WHERE s.tenant_id=NEW.tenant_id AND s.book_id=NEW.book_id AND s.amount_minor<>0 AND s.source_date BETWEEN NEW.start_date AND NEW.end_date AND NOT EXISTS(
 SELECT 1 FROM archava_accounting.stock_journal_sources x JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id WHERE x.tenant_id=s.tenant_id AND x.source_id=s.id AND j.state='posted')) THEN RAISE EXCEPTION 'Resolve unposted stock cost sources before closing' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER stock_source_close_guard BEFORE UPDATE ON archava_accounting.periods FOR EACH ROW EXECUTE FUNCTION archava_accounting.stock_close_guard();
CREATE FUNCTION archava_accounting.stock_reversal_guard() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF NEW.reversal_of IS NOT NULL AND EXISTS(SELECT 1 FROM archava_accounting.stock_journal_sources WHERE tenant_id=NEW.tenant_id AND journal_id=NEW.reversal_of) THEN RAISE EXCEPTION 'Correct stock through its original operating workflow' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER stock_source_reversal_guard BEFORE INSERT ON archava_accounting.journals FOR EACH ROW EXECUTE FUNCTION archava_accounting.stock_reversal_guard();

-- Existing source metadata attachment never posts money or guesses an opening.
DO $$
DECLARE tenants text[];scope text;prior_scope text:=current_setting('archava.tenant_id',true);layer record;
BEGIN
 -- Serialize with existing writes and fresh writer claims before taking any
 -- source/book locks. Existing runtime epochs remain valid for this additive DDL.
 PERFORM pg_advisory_xact_lock(hashtext('archava-serving-writer-takeover'));
 IF to_regclass('archava_control.writer_lease') IS NOT NULL THEN
  PERFORM 1 FROM archava_control.writer_lease WHERE lease_name='archava-serving' FOR UPDATE;
 END IF;
 LOCK TABLE archava_accounting.purchase_sources IN ACCESS EXCLUSIVE MODE;
 IF NOT (SELECT relforcerowsecurity FROM pg_class WHERE oid='archava_accounting.purchase_sources'::regclass) THEN RAISE EXCEPTION 'Existing source store must retain forced tenant RLS' USING ERRCODE='55000';END IF;
 ALTER TABLE archava_accounting.purchase_sources NO FORCE ROW LEVEL SECURITY;
 SELECT coalesce(array_agg(DISTINCT tenant_id ORDER BY tenant_id),ARRAY[]::text[]) INTO tenants FROM archava_accounting.purchase_sources;
 ALTER TABLE archava_accounting.purchase_sources FORCE ROW LEVEL SECURITY;
 FOREACH scope IN ARRAY tenants LOOP
  PERFORM set_config('archava.tenant_id',scope,true);
  INSERT INTO archava_accounting.stock_layer_origins(tenant_id,layer_id,book_id,inventory_account_id,purchase_source_id)
  SELECT l.tenant_id,l.id,s.book_id,(m->>'accountId')::uuid,s.id FROM archava_commerce.inventory_layers l JOIN archava_accounting.purchase_sources s ON s.tenant_id=l.tenant_id AND s.id=l.source_request_id AND s.effect='receipt' JOIN archava_accounting.purchase_origins o ON o.tenant_id=s.tenant_id AND o.id=s.origin_id CROSS JOIN LATERAL jsonb_array_elements(o.policy_snapshot->'mappings') m
  WHERE l.tenant_id=scope AND l.source_kind='purchase' AND m->>'role'='inventory' AND NOT EXISTS(SELECT 1 FROM archava_accounting.stock_layer_origins b WHERE b.tenant_id=l.tenant_id AND b.layer_id=l.id);
  -- A returned layer is newer than the exact original layer it consumed.
  -- NULL historical references are deliberately excluded, never matched by cost.
  FOR layer IN SELECT * FROM archava_commerce.inventory_layers WHERE tenant_id=scope AND source_kind='return' AND return_cost_id IS NOT NULL ORDER BY id LOOP
   INSERT INTO archava_accounting.stock_layer_origins(tenant_id,layer_id,book_id,inventory_account_id,purchase_source_id,parent_layer_id)
   SELECT layer.tenant_id,layer.id,p.book_id,p.inventory_account_id,p.purchase_source_id,p.layer_id FROM archava_commerce.inventory_return_costs r JOIN archava_commerce.inventory_uses u ON u.tenant_id=r.tenant_id AND u.id=r.source_use_id JOIN archava_accounting.stock_layer_origins p ON p.tenant_id=u.tenant_id AND p.layer_id=u.layer_id
   WHERE r.tenant_id=scope AND r.id=layer.return_cost_id AND NOT EXISTS(SELECT 1 FROM archava_accounting.stock_layer_origins b WHERE b.tenant_id=scope AND b.layer_id=layer.id);
  END LOOP;
 END LOOP;
 PERFORM set_config('archava.tenant_id',coalesce(prior_scope,''),true);
END $$;

CREATE FUNCTION archava_accounting.stock_guards_complete() RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog AS $$
 SELECT archava_commerce.return_layer_guards_complete()
 AND NOT EXISTS(SELECT 1 FROM (VALUES
 ('archava_accounting','stock_layer_origins','stock_layer_origin_guard','archava_accounting.guard_stock_layer_origin()',7,false,false,''),
 ('archava_accounting','stock_layer_origins','stock_layer_origin_immutable','archava_accounting.immutable()',27,false,false,''),
 ('archava_accounting','purchase_sources','bind_purchase_layers','archava_accounting.bind_purchase_layers()',5,false,false,''),
 ('archava_commerce','inventory_layers','bind_returned_layer','archava_accounting.bind_returned_layer()',5,false,false,''),
 ('archava_commerce','orders','aa_stock_source_boundary','archava_accounting.prepare_stock_source()',19,false,false,''),
 ('archava_commerce','commands','capture_financial_stock_source','archava_accounting.capture_stock_source()',17,false,false,'5'),
 ('archava_accounting','stock_policies','stock_policy_guard','archava_accounting.guard_stock_policy()',31,false,false,''),
 ('archava_accounting','stock_policies','stock_policy_complete','archava_accounting.assert_stock_policy()',21,true,true,''),
 ('archava_accounting','stock_sources','stock_source_guard','archava_accounting.guard_stock_source()',7,false,false,''),
 ('archava_accounting','stock_sources','stock_source_immutable','archava_accounting.immutable()',27,false,false,''),
 ('archava_accounting','stock_journal_sources','stock_journal_source_guard','archava_accounting.guard_stock_journal_source()',7,false,false,''),
 ('archava_accounting','stock_journal_sources','stock_journal_source_immutable','archava_accounting.immutable()',27,false,false,''),
 ('archava_accounting','journals','stock_journal_claim_complete','archava_accounting.assert_stock_journal_claim()',5,true,true,''),
 ('archava_accounting','stock_journal_sources','stock_link_claim_complete','archava_accounting.assert_stock_journal_claim()',5,true,true,''),
 ('archava_accounting','periods','stock_source_close_guard','archava_accounting.stock_close_guard()',19,false,false,''),
 ('archava_accounting','journals','stock_source_reversal_guard','archava_accounting.stock_reversal_guard()',7,false,false,'')
 ) AS required(schema_name,table_name,trigger_name,function_name,trigger_type,is_deferred,initially_deferred,columns)
 WHERE NOT EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid=to_regclass(format('%I.%I',required.schema_name,required.table_name)) AND t.tgname=required.trigger_name
  AND t.tgfoid=to_regprocedure(required.function_name) AND t.tgtype=required.trigger_type AND t.tgenabled='O' AND NOT t.tgisinternal
  AND t.tgdeferrable=required.is_deferred AND t.tginitdeferred=required.initially_deferred AND t.tgqual IS NULL AND t.tgattr=required.columns::int2vector AND encode(t.tgargs,'hex')=''))
 AND NOT has_table_privilege(session_user,'archava_accounting.stock_layer_origins','INSERT,UPDATE,DELETE')
 AND NOT has_function_privilege(session_user,'archava_accounting.bind_purchase_layers()','EXECUTE')
 AND NOT has_function_privilege(session_user,'archava_accounting.bind_returned_layer()','EXECUTE')
 AND (SELECT bool_and(p.prosecdef AND p.proconfig=ARRAY['search_path=pg_catalog']) FROM pg_proc p WHERE p.oid IN('archava_accounting.bind_purchase_layers()'::regprocedure,'archava_accounting.bind_returned_layer()'::regprocedure))
$$;
REVOKE ALL ON FUNCTION archava_accounting.stock_guards_complete() FROM PUBLIC;

DO $$ BEGIN
 IF to_regprocedure('archava_control.install_runtime_write_guards()') IS NOT NULL THEN
  PERFORM archava_control.install_runtime_write_guards();
 END IF;
END $$;
