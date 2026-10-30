CREATE TABLE archava_purchasing.memberships (
 tenant_id text NOT NULL,principal_id text NOT NULL,kind text NOT NULL CHECK(kind IN ('human','service')),
 permissions text[] NOT NULL CHECK(permissions <@ ARRAY['suppliers.read','suppliers.write','offers.read','offers.write','orders.read','orders.write','receipts.read','receipts.write','audit.read']::text[]),
 scope text NOT NULL CHECK(scope IN ('all','assigned')),active boolean NOT NULL DEFAULT true,PRIMARY KEY(tenant_id,principal_id)
);
CREATE TABLE archava_purchasing.suppliers (
 tenant_id text NOT NULL,id uuid NOT NULL,code text NOT NULL CHECK(code ~ '^[A-Z0-9][A-Z0-9_-]{0,39}$'),name text NOT NULL CHECK(length(name) BETWEEN 1 AND 160),active boolean NOT NULL DEFAULT true,
 revision integer NOT NULL DEFAULT 1 CHECK(revision>0),created_at timestamptz NOT NULL DEFAULT clock_timestamp(),updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,code)
);
CREATE TABLE archava_purchasing.offers (
 tenant_id text NOT NULL,id uuid NOT NULL,supplier_id uuid NOT NULL,product_id uuid NOT NULL,unit_cost integer NOT NULL CHECK(unit_cost BETWEEN 0 AND 1000000000),currency text NOT NULL DEFAULT 'IDR' CHECK(currency='IDR'),active boolean NOT NULL DEFAULT true,
 revision integer NOT NULL DEFAULT 1 CHECK(revision>0),created_at timestamptz NOT NULL DEFAULT clock_timestamp(),updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,supplier_id,product_id),FOREIGN KEY(tenant_id,supplier_id) REFERENCES archava_purchasing.suppliers(tenant_id,id),FOREIGN KEY(tenant_id,product_id) REFERENCES archava_commerce.products(tenant_id,id)
);
CREATE TABLE archava_purchasing.orders (
 tenant_id text NOT NULL,id uuid NOT NULL,owner_id text NOT NULL,supplier_id uuid NOT NULL,supplier_code text NOT NULL,supplier_name text NOT NULL CHECK(length(supplier_name) BETWEEN 1 AND 160),state text NOT NULL DEFAULT 'draft' CHECK(state IN ('draft','confirmed','partial','received','cancelled','short_closed')),
 total_estimate bigint NOT NULL CHECK(total_estimate BETWEEN 0 AND 500000000000000),currency text NOT NULL DEFAULT 'IDR' CHECK(currency='IDR'),confirmed_at timestamptz,
 revision integer NOT NULL DEFAULT 1 CHECK(revision>0),created_at timestamptz NOT NULL DEFAULT clock_timestamp(),updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),PRIMARY KEY(tenant_id,id),FOREIGN KEY(tenant_id,owner_id) REFERENCES archava_purchasing.memberships(tenant_id,principal_id),FOREIGN KEY(tenant_id,supplier_id) REFERENCES archava_purchasing.suppliers(tenant_id,id),CHECK(state IN ('draft','cancelled') OR confirmed_at IS NOT NULL),CHECK(confirmed_at IS NULL OR isfinite(confirmed_at))
);
CREATE TABLE archava_purchasing.lines (
 tenant_id text NOT NULL,id uuid NOT NULL,order_id uuid NOT NULL,offer_id uuid NOT NULL,offer_revision integer NOT NULL CHECK(offer_revision>0),product_id uuid NOT NULL,sku text NOT NULL,title text NOT NULL,unit text NOT NULL CHECK(unit IN ('unit','gram','millilitre')),
 quantity integer NOT NULL CHECK(quantity BETWEEN 1 AND 1000000),unit_cost integer NOT NULL CHECK(unit_cost BETWEEN 0 AND 1000000000),accepted_quantity integer NOT NULL DEFAULT 0 CHECK(accepted_quantity>=0),rejected_quantity integer NOT NULL DEFAULT 0 CHECK(rejected_quantity>=0),
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,order_id,id),UNIQUE(tenant_id,order_id,product_id),FOREIGN KEY(tenant_id,order_id) REFERENCES archava_purchasing.orders(tenant_id,id),FOREIGN KEY(tenant_id,offer_id) REFERENCES archava_purchasing.offers(tenant_id,id),FOREIGN KEY(tenant_id,product_id) REFERENCES archava_commerce.products(tenant_id,id),CHECK(accepted_quantity+rejected_quantity<=quantity)
);
CREATE TABLE archava_purchasing.commands (
 tenant_id text NOT NULL,request_id uuid NOT NULL,actor_id text NOT NULL,fingerprint text NOT NULL,receipt jsonb,created_at timestamptz NOT NULL DEFAULT clock_timestamp(),PRIMARY KEY(tenant_id,request_id),FOREIGN KEY(tenant_id,actor_id) REFERENCES archava_purchasing.memberships(tenant_id,principal_id)
);
CREATE TABLE archava_purchasing.receipts (
 tenant_id text NOT NULL,request_id uuid NOT NULL,order_id uuid NOT NULL,actor_id text NOT NULL,revision integer NOT NULL CHECK(revision>0),reason text NOT NULL CHECK(length(reason) BETWEEN 1 AND 2000),occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),PRIMARY KEY(tenant_id,request_id),FOREIGN KEY(tenant_id,request_id) REFERENCES archava_purchasing.commands(tenant_id,request_id),FOREIGN KEY(tenant_id,order_id) REFERENCES archava_purchasing.orders(tenant_id,id)
);
CREATE TABLE archava_purchasing.receipt_lines (
 tenant_id text NOT NULL,request_id uuid NOT NULL,order_id uuid NOT NULL,line_id uuid NOT NULL,product_id uuid NOT NULL,accepted integer NOT NULL CHECK(accepted BETWEEN 0 AND 1000000),rejected integer NOT NULL CHECK(rejected BETWEEN 0 AND 1000000),PRIMARY KEY(tenant_id,request_id,line_id),FOREIGN KEY(tenant_id,request_id) REFERENCES archava_purchasing.receipts(tenant_id,request_id),FOREIGN KEY(tenant_id,order_id,line_id) REFERENCES archava_purchasing.lines(tenant_id,order_id,id),CHECK(accepted+rejected BETWEEN 1 AND 1000000)
);
CREATE TABLE archava_purchasing.audit (
 tenant_id text NOT NULL,request_id uuid NOT NULL,actor_id text NOT NULL,operation text NOT NULL,record_id uuid NOT NULL,revision integer NOT NULL CHECK(revision>0),reason text CHECK(length(reason) BETWEEN 1 AND 2000),occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),PRIMARY KEY(tenant_id,request_id),FOREIGN KEY(tenant_id,request_id) REFERENCES archava_purchasing.commands(tenant_id,request_id)
);
CREATE INDEX purchase_owner ON archava_purchasing.orders(tenant_id,owner_id,id);
CREATE INDEX purchase_lines ON archava_purchasing.lines(tenant_id,order_id,id);
CREATE INDEX purchase_receipts ON archava_purchasing.receipts(tenant_id,order_id,occurred_at DESC,request_id);
CREATE INDEX purchase_history ON archava_purchasing.audit(tenant_id,record_id,occurred_at DESC,request_id);
CREATE FUNCTION archava_purchasing.guard_supplier() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.id IS DISTINCT FROM OLD.id OR NEW.code IS DISTINCT FROM OLD.code OR NEW.name IS DISTINCT FROM OLD.name OR NEW.created_at IS DISTINCT FROM OLD.created_at OR NEW.revision<>OLD.revision+1 THEN RAISE EXCEPTION 'Supplier identity is immutable' USING ERRCODE='23514';END IF;RETURN NEW;END $$;
CREATE TRIGGER supplier_guard BEFORE UPDATE ON archava_purchasing.suppliers FOR EACH ROW EXECUTE FUNCTION archava_purchasing.guard_supplier();
CREATE FUNCTION archava_purchasing.guard_offer() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.id IS DISTINCT FROM OLD.id OR NEW.supplier_id IS DISTINCT FROM OLD.supplier_id OR NEW.product_id IS DISTINCT FROM OLD.product_id OR NEW.currency IS DISTINCT FROM OLD.currency OR NEW.created_at IS DISTINCT FROM OLD.created_at OR NEW.revision<>OLD.revision+1 THEN RAISE EXCEPTION 'Offer identity is immutable' USING ERRCODE='23514';END IF;RETURN NEW;END $$;
CREATE TRIGGER offer_guard BEFORE UPDATE ON archava_purchasing.offers FOR EACH ROW EXECUTE FUNCTION archava_purchasing.guard_offer();
CREATE FUNCTION archava_purchasing.guard_order() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF TG_OP='INSERT' THEN IF NEW.state<>'draft' OR NEW.confirmed_at IS NOT NULL THEN RAISE EXCEPTION 'Purchase must start as draft' USING ERRCODE='23514';END IF;
 ELSE
  IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.id IS DISTINCT FROM OLD.id OR NEW.owner_id IS DISTINCT FROM OLD.owner_id OR NEW.supplier_id IS DISTINCT FROM OLD.supplier_id OR NEW.supplier_code IS DISTINCT FROM OLD.supplier_code OR NEW.supplier_name IS DISTINCT FROM OLD.supplier_name OR NEW.currency IS DISTINCT FROM OLD.currency OR NEW.created_at IS DISTINCT FROM OLD.created_at OR NEW.revision<>OLD.revision+1 OR (OLD.state<>'draft' AND NEW.total_estimate IS DISTINCT FROM OLD.total_estimate) OR (OLD.confirmed_at IS NOT NULL AND NEW.confirmed_at IS DISTINCT FROM OLD.confirmed_at) THEN RAISE EXCEPTION 'Confirmed purchase is immutable' USING ERRCODE='23514';END IF;
  IF NOT ((OLD.state='draft' AND NEW.state IN ('draft','confirmed','cancelled')) OR (OLD.state='confirmed' AND NEW.state IN ('partial','received','cancelled','short_closed')) OR (OLD.state='partial' AND NEW.state IN ('partial','received','short_closed'))) THEN RAISE EXCEPTION 'Purchase transition refused' USING ERRCODE='23514';END IF;
 END IF;RETURN NEW;
END $$;
CREATE TRIGGER order_guard BEFORE INSERT OR UPDATE ON archava_purchasing.orders FOR EACH ROW EXECUTE FUNCTION archava_purchasing.guard_order();
CREATE FUNCTION archava_purchasing.guard_line() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE t text;p uuid;parent text;
BEGIN
 IF TG_OP='DELETE' THEN t:=OLD.tenant_id;p:=OLD.order_id;ELSE t:=NEW.tenant_id;p:=NEW.order_id;END IF;
 SELECT state INTO parent FROM archava_purchasing.orders WHERE tenant_id=t AND id=p FOR UPDATE;
 IF TG_OP IN ('INSERT','DELETE') THEN
  IF parent IS DISTINCT FROM 'draft' THEN RAISE EXCEPTION 'Confirmed lines are immutable' USING ERRCODE='23514';END IF;
  IF TG_OP='DELETE' THEN RETURN OLD;END IF;
  IF NEW.accepted_quantity<>0 OR NEW.rejected_quantity<>0 THEN RAISE EXCEPTION 'New line has receipt totals' USING ERRCODE='23514';END IF;RETURN NEW;
 END IF;
 IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.id IS DISTINCT FROM OLD.id OR NEW.order_id IS DISTINCT FROM OLD.order_id OR NEW.offer_id IS DISTINCT FROM OLD.offer_id OR NEW.offer_revision IS DISTINCT FROM OLD.offer_revision OR NEW.product_id IS DISTINCT FROM OLD.product_id OR NEW.sku IS DISTINCT FROM OLD.sku OR NEW.title IS DISTINCT FROM OLD.title OR NEW.unit IS DISTINCT FROM OLD.unit OR NEW.quantity IS DISTINCT FROM OLD.quantity OR NEW.unit_cost IS DISTINCT FROM OLD.unit_cost OR parent NOT IN ('confirmed','partial')
 OR NEW.accepted_quantity IS DISTINCT FROM (SELECT coalesce(sum(accepted),0) FROM archava_purchasing.receipt_lines WHERE tenant_id=t AND line_id=NEW.id) OR NEW.rejected_quantity IS DISTINCT FROM (SELECT coalesce(sum(rejected),0) FROM archava_purchasing.receipt_lines WHERE tenant_id=t AND line_id=NEW.id)
 THEN RAISE EXCEPTION 'Line quote or receipt totals refused' USING ERRCODE='23514';END IF;RETURN NEW;
END $$;
CREATE TRIGGER line_guard BEFORE INSERT OR UPDATE OR DELETE ON archava_purchasing.lines FOR EACH ROW EXECUTE FUNCTION archava_purchasing.guard_line();
CREATE FUNCTION archava_purchasing.guard_receipt_line() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE l archava_purchasing.lines;parent text;received bigint;
BEGIN
 IF TG_OP<>'INSERT' THEN RAISE EXCEPTION 'Receipt findings are immutable';END IF;
 SELECT state INTO parent FROM archava_purchasing.orders WHERE tenant_id=NEW.tenant_id AND id=NEW.order_id FOR UPDATE;
 SELECT * INTO l FROM archava_purchasing.lines WHERE tenant_id=NEW.tenant_id AND id=NEW.line_id AND order_id=NEW.order_id;
 SELECT coalesce(sum(accepted+rejected),0) INTO received FROM archava_purchasing.receipt_lines WHERE tenant_id=NEW.tenant_id AND line_id=NEW.line_id;
 IF parent IS NULL OR parent NOT IN ('confirmed','partial') OR (SELECT order_id FROM archava_purchasing.receipts WHERE tenant_id=NEW.tenant_id AND request_id=NEW.request_id) IS DISTINCT FROM NEW.order_id OR l.id IS NULL OR l.product_id<>NEW.product_id OR received+NEW.accepted+NEW.rejected>l.quantity THEN RAISE EXCEPTION 'Receipt exceeds ordered quantity' USING ERRCODE='23514';END IF;RETURN NEW;
END $$;
CREATE TRIGGER receipt_line_guard BEFORE INSERT OR UPDATE OR DELETE ON archava_purchasing.receipt_lines FOR EACH ROW EXECUTE FUNCTION archava_purchasing.guard_receipt_line();
CREATE FUNCTION archava_purchasing.assert_entity() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE n integer;current_revision integer;
BEGIN
 EXECUTE format('SELECT revision FROM archava_purchasing.%I WHERE tenant_id=$1 AND id=$2',TG_TABLE_NAME) INTO current_revision USING NEW.tenant_id,NEW.id;
 SELECT count(*) INTO n FROM archava_purchasing.audit WHERE tenant_id=NEW.tenant_id AND record_id=NEW.id;
 IF n<>current_revision OR NOT EXISTS(SELECT 1 FROM archava_purchasing.audit WHERE tenant_id=NEW.tenant_id AND record_id=NEW.id AND revision=NEW.revision) THEN RAISE EXCEPTION 'Purchase revision has no audit' USING ERRCODE='23514';END IF;RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER supplier_complete AFTER INSERT OR UPDATE ON archava_purchasing.suppliers DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_purchasing.assert_entity();
CREATE CONSTRAINT TRIGGER offer_complete AFTER INSERT OR UPDATE ON archava_purchasing.offers DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_purchasing.assert_entity();
CREATE CONSTRAINT TRIGGER order_revision_complete AFTER INSERT OR UPDATE ON archava_purchasing.orders DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_purchasing.assert_entity();
CREATE FUNCTION archava_purchasing.assert_order() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE o archava_purchasing.orders;n integer;total bigint;accounted bigint;ordered bigint;
BEGIN
 SELECT * INTO o FROM archava_purchasing.orders WHERE tenant_id=NEW.tenant_id AND id=NEW.id;
 SELECT count(*),coalesce(sum(quantity::bigint*unit_cost),0),coalesce(sum(accepted_quantity+rejected_quantity),0),coalesce(sum(quantity),0) INTO n,total,accounted,ordered FROM archava_purchasing.lines WHERE tenant_id=NEW.tenant_id AND order_id=NEW.id;
 IF n NOT BETWEEN 1 AND 50 OR total<>o.total_estimate OR (o.state IN ('draft','confirmed','cancelled') AND accounted<>0) OR (o.state='partial' AND (accounted=0 OR accounted>=ordered)) OR (o.state='received' AND accounted<>ordered) THEN RAISE EXCEPTION 'Purchase quote or receipt state is incomplete' USING ERRCODE='23514';END IF;
 IF EXISTS(SELECT 1 FROM archava_purchasing.lines l WHERE l.tenant_id=NEW.tenant_id AND l.order_id=NEW.id AND (l.accepted_quantity<>(SELECT coalesce(sum(accepted),0) FROM archava_purchasing.receipt_lines WHERE tenant_id=l.tenant_id AND line_id=l.id) OR l.rejected_quantity<>(SELECT coalesce(sum(rejected),0) FROM archava_purchasing.receipt_lines WHERE tenant_id=l.tenant_id AND line_id=l.id))) THEN RAISE EXCEPTION 'Purchase receipt balance is incomplete' USING ERRCODE='23514';END IF;RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER order_complete AFTER INSERT OR UPDATE ON archava_purchasing.orders DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_purchasing.assert_order();
CREATE FUNCTION archava_purchasing.assert_receipt() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE c archava_commerce.commands;a archava_purchasing.audit;n integer;positive integer;movements integer;
BEGIN
 SELECT count(*),count(*) FILTER(WHERE accepted>0) INTO n,positive FROM archava_purchasing.receipt_lines WHERE tenant_id=NEW.tenant_id AND request_id=NEW.request_id;
 SELECT * INTO c FROM archava_commerce.commands WHERE tenant_id=NEW.tenant_id AND request_id=NEW.request_id;
 SELECT * INTO a FROM archava_purchasing.audit WHERE tenant_id=NEW.tenant_id AND request_id=NEW.request_id;
 SELECT count(*) INTO movements FROM archava_commerce.movements WHERE tenant_id=NEW.tenant_id AND request_id=NEW.request_id;
 IF n NOT BETWEEN 1 AND 50 OR c.request_id IS NULL OR c.actor_id<>NEW.actor_id OR c.receipt->>'domain' IS DISTINCT FROM 'purchasing' OR (c.receipt->>'id')::uuid IS DISTINCT FROM NEW.order_id OR (c.receipt->>'revision')::integer IS DISTINCT FROM NEW.revision OR a.actor_id IS DISTINCT FROM NEW.actor_id OR a.record_id IS DISTINCT FROM NEW.order_id OR a.revision IS DISTINCT FROM NEW.revision OR a.operation IS DISTINCT FROM 'order.receive' OR movements<>positive THEN RAISE EXCEPTION 'Physical receipt has no complete cross-domain audit' USING ERRCODE='23514';END IF;
 IF EXISTS(SELECT 1 FROM archava_purchasing.receipt_lines l WHERE l.tenant_id=NEW.tenant_id AND l.request_id=NEW.request_id AND l.accepted>0 AND NOT EXISTS(SELECT 1 FROM archava_commerce.movements m WHERE m.tenant_id=l.tenant_id AND m.request_id=l.request_id AND m.kind='purchase' AND m.purchase_id=l.order_id AND m.product_id=l.product_id AND m.quantity=l.accepted)) THEN RAISE EXCEPTION 'Accepted receipt has no exact stock movement' USING ERRCODE='23514';END IF;RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER receipt_complete AFTER INSERT ON archava_purchasing.receipts DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_purchasing.assert_receipt();
CREATE FUNCTION archava_purchasing.immutable_record() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'Purchasing history is append-only';END $$;
CREATE TRIGGER audit_immutable BEFORE UPDATE OR DELETE ON archava_purchasing.audit FOR EACH ROW EXECUTE FUNCTION archava_purchasing.immutable_record();
CREATE TRIGGER receipt_immutable BEFORE UPDATE OR DELETE ON archava_purchasing.receipts FOR EACH ROW EXECUTE FUNCTION archava_purchasing.immutable_record();
CREATE FUNCTION archava_purchasing.guard_command() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF TG_OP='DELETE' OR OLD.receipt IS NOT NULL OR NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.request_id IS DISTINCT FROM OLD.request_id OR NEW.actor_id IS DISTINCT FROM OLD.actor_id OR NEW.fingerprint IS DISTINCT FROM OLD.fingerprint OR NEW.created_at IS DISTINCT FROM OLD.created_at THEN RAISE EXCEPTION 'Command receipt is immutable';END IF;RETURN NEW;
END $$;
CREATE TRIGGER command_immutable BEFORE UPDATE OR DELETE ON archava_purchasing.commands FOR EACH ROW EXECUTE FUNCTION archava_purchasing.guard_command();
CREATE FUNCTION archava_purchasing.assert_command() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE c archava_purchasing.commands;a archava_purchasing.audit;
BEGIN
 SELECT * INTO c FROM archava_purchasing.commands WHERE tenant_id=NEW.tenant_id AND request_id=NEW.request_id;
 SELECT * INTO a FROM archava_purchasing.audit WHERE tenant_id=NEW.tenant_id AND request_id=NEW.request_id;
 IF c.receipt IS NULL OR a.request_id IS NULL OR a.actor_id<>c.actor_id OR a.operation IS DISTINCT FROM (c.receipt->>'operation') OR a.record_id IS DISTINCT FROM (c.receipt->>'id')::uuid OR a.revision IS DISTINCT FROM (c.receipt->>'revision')::integer OR (c.receipt->>'requestId')::uuid IS DISTINCT FROM c.request_id THEN RAISE EXCEPTION 'Command audit binding refused' USING ERRCODE='23514';END IF;RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER command_complete AFTER INSERT OR UPDATE ON archava_purchasing.commands DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_purchasing.assert_command();
DO $$ DECLARE owned text;BEGIN
 FOREACH owned IN ARRAY ARRAY['memberships','suppliers','offers','orders','lines','receipts','receipt_lines','commands','audit'] LOOP
  EXECUTE format('ALTER TABLE archava_purchasing.%I ENABLE ROW LEVEL SECURITY',owned);
  EXECUTE format('ALTER TABLE archava_purchasing.%I FORCE ROW LEVEL SECURITY',owned);
  EXECUTE format('CREATE POLICY purchase_tenant ON archava_purchasing.%I USING(tenant_id=current_setting(''archava.tenant_id'',true)) WITH CHECK(tenant_id=current_setting(''archava.tenant_id'',true))',owned);
 END LOOP;
END $$;
CREATE FUNCTION archava_purchasing.lock_membership(wanted_tenant text,wanted_principal text) RETURNS SETOF archava_purchasing.memberships LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog AS $$
 SELECT * FROM archava_purchasing.memberships WHERE tenant_id=wanted_tenant AND principal_id=wanted_principal AND wanted_tenant=current_setting('archava.tenant_id',true) FOR SHARE
$$;
REVOKE ALL ON FUNCTION archava_purchasing.lock_membership(text,text) FROM PUBLIC;
