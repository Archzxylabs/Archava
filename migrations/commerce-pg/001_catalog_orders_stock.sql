CREATE TABLE archava_commerce.memberships (
 tenant_id text NOT NULL, principal_id text NOT NULL, kind text NOT NULL CHECK(kind IN ('human','service')),
 permissions text[] NOT NULL CHECK(permissions <@ ARRAY['catalog.read','catalog.write','stock.read','stock.write','orders.read','orders.write','returns.write','audit.read']::text[]),
 scope text NOT NULL CHECK(scope IN ('all','assigned')), active boolean NOT NULL DEFAULT true, PRIMARY KEY(tenant_id,principal_id)
);
CREATE TABLE archava_commerce.products (
 tenant_id text NOT NULL,id uuid NOT NULL,sku text NOT NULL CHECK(sku ~ '^[A-Z0-9][A-Z0-9_-]{0,39}$'), title text NOT NULL CHECK(length(title) BETWEEN 1 AND 160),
 unit text NOT NULL CHECK(unit IN ('unit','gram','millilitre')),track_stock boolean NOT NULL,price integer NOT NULL CHECK(price BETWEEN 0 AND 1000000000),currency text NOT NULL DEFAULT 'IDR' CHECK(currency='IDR'),active boolean NOT NULL DEFAULT true,
 revision integer NOT NULL DEFAULT 1 CHECK(revision>0),created_at timestamptz NOT NULL DEFAULT clock_timestamp(),updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,sku)
);
CREATE TABLE archava_commerce.stock_balances (
 tenant_id text NOT NULL,product_id uuid NOT NULL,quantity integer NOT NULL DEFAULT 0 CHECK(quantity BETWEEN 0 AND 1000000000),revision integer NOT NULL DEFAULT 1 CHECK(revision>0),
 PRIMARY KEY(tenant_id,product_id),FOREIGN KEY(tenant_id,product_id) REFERENCES archava_commerce.products(tenant_id,id)
);
CREATE TABLE archava_commerce.orders (
 tenant_id text NOT NULL,id uuid NOT NULL,owner_id text NOT NULL,customer_name text NOT NULL CHECK(length(customer_name) BETWEEN 1 AND 160),
 state text NOT NULL DEFAULT 'draft' CHECK(state IN ('draft','reserved','fulfilled','cancelled','returned')),managed_by text CHECK(managed_by='foodservice'),
 total_estimate bigint NOT NULL CHECK(total_estimate BETWEEN 0 AND 500000000000000),currency text NOT NULL DEFAULT 'IDR' CHECK(currency='IDR'),expires_at timestamptz,fulfilled_at timestamptz,
 revision integer NOT NULL DEFAULT 1 CHECK(revision>0),created_at timestamptz NOT NULL DEFAULT clock_timestamp(),updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,id),FOREIGN KEY(tenant_id,owner_id) REFERENCES archava_commerce.memberships(tenant_id,principal_id),
 CHECK(state<>'reserved' OR (expires_at IS NOT NULL AND isfinite(expires_at))),CHECK((state IN ('fulfilled','returned'))=(fulfilled_at IS NOT NULL))
);
CREATE TABLE archava_commerce.order_lines (
 tenant_id text NOT NULL,id uuid NOT NULL,order_id uuid NOT NULL,product_id uuid NOT NULL,sku text NOT NULL,title text NOT NULL,unit text NOT NULL CHECK(unit IN ('unit','gram','millilitre')),track_stock boolean NOT NULL,
 quantity integer NOT NULL CHECK(quantity BETWEEN 1 AND 10000),price integer NOT NULL CHECK(price BETWEEN 0 AND 1000000000),product_revision integer NOT NULL CHECK(product_revision>0),returned_quantity integer NOT NULL DEFAULT 0 CHECK(returned_quantity BETWEEN 0 AND quantity),
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,order_id,product_id),UNIQUE(tenant_id,order_id,id),
 FOREIGN KEY(tenant_id,order_id) REFERENCES archava_commerce.orders(tenant_id,id),FOREIGN KEY(tenant_id,product_id) REFERENCES archava_commerce.products(tenant_id,id)
);
CREATE TABLE archava_commerce.allocations (
 tenant_id text NOT NULL,order_id uuid NOT NULL,product_id uuid NOT NULL,quantity integer NOT NULL CHECK(quantity BETWEEN 1 AND 1000000000),
 state text NOT NULL CHECK(state IN ('held','consumed','released')),expires_at timestamptz NOT NULL CHECK(isfinite(expires_at)),
 PRIMARY KEY(tenant_id,order_id,product_id),FOREIGN KEY(tenant_id,order_id) REFERENCES archava_commerce.orders(tenant_id,id),FOREIGN KEY(tenant_id,product_id) REFERENCES archava_commerce.products(tenant_id,id)
);
CREATE TABLE archava_commerce.commands (
 tenant_id text NOT NULL,request_id uuid NOT NULL,actor_id text NOT NULL,fingerprint text NOT NULL,receipt jsonb,created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,request_id),FOREIGN KEY(tenant_id,actor_id) REFERENCES archava_commerce.memberships(tenant_id,principal_id)
);
CREATE TABLE archava_commerce.movements (
 tenant_id text NOT NULL,request_id uuid NOT NULL,product_id uuid NOT NULL,order_id uuid,kind text NOT NULL CHECK(kind IN ('adjust','consume','return')),quantity integer NOT NULL CHECK(quantity<>0 AND quantity BETWEEN -1000000000 AND 1000000000),reason text CHECK(length(reason) BETWEEN 1 AND 2000),occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,request_id,product_id,kind),FOREIGN KEY(tenant_id,request_id) REFERENCES archava_commerce.commands(tenant_id,request_id),FOREIGN KEY(tenant_id,product_id) REFERENCES archava_commerce.products(tenant_id,id),FOREIGN KEY(tenant_id,order_id) REFERENCES archava_commerce.orders(tenant_id,id),
 CHECK((kind='adjust')=(order_id IS NULL)),CHECK(kind<>'adjust' OR reason IS NOT NULL),CHECK(kind<>'consume' OR quantity<0),CHECK(kind<>'return' OR (quantity>0 AND reason IS NOT NULL))
);
CREATE TABLE archava_commerce.returns (
 tenant_id text NOT NULL,request_id uuid NOT NULL,order_id uuid NOT NULL,line_id uuid NOT NULL,quantity integer NOT NULL CHECK(quantity BETWEEN 1 AND 10000),disposition text NOT NULL CHECK(disposition IN ('restock','discard')),reason text NOT NULL CHECK(length(reason) BETWEEN 1 AND 2000),occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,request_id,line_id),FOREIGN KEY(tenant_id,request_id) REFERENCES archava_commerce.commands(tenant_id,request_id),FOREIGN KEY(tenant_id,order_id,line_id) REFERENCES archava_commerce.order_lines(tenant_id,order_id,id)
);
CREATE TABLE archava_commerce.audit (
 tenant_id text NOT NULL,request_id uuid NOT NULL,actor_id text NOT NULL,operation text NOT NULL,record_id uuid NOT NULL,revision integer NOT NULL CHECK(revision>0),reason text CHECK(length(reason) BETWEEN 1 AND 2000),occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,request_id),FOREIGN KEY(tenant_id,request_id) REFERENCES archava_commerce.commands(tenant_id,request_id)
);
CREATE INDEX orders_owner ON archava_commerce.orders(tenant_id,owner_id,id);
CREATE INDEX order_lines_parent ON archava_commerce.order_lines(tenant_id,order_id,id);
CREATE INDEX active_stock_holds ON archava_commerce.allocations(tenant_id,product_id,expires_at) WHERE state='held';
CREATE INDEX movement_history ON archava_commerce.movements(tenant_id,product_id,occurred_at DESC,request_id);
CREATE INDEX return_history ON archava_commerce.returns(tenant_id,order_id,occurred_at DESC,request_id);
CREATE INDEX commerce_history ON archava_commerce.audit(tenant_id,record_id,occurred_at DESC,request_id);

CREATE FUNCTION archava_commerce.init_stock() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog AS $$
BEGIN INSERT INTO archava_commerce.stock_balances(tenant_id,product_id) VALUES(NEW.tenant_id,NEW.id);RETURN NEW;END $$;
REVOKE ALL ON FUNCTION archava_commerce.init_stock() FROM PUBLIC;
CREATE TRIGGER product_stock AFTER INSERT ON archava_commerce.products FOR EACH ROW EXECUTE FUNCTION archava_commerce.init_stock();
CREATE FUNCTION archava_commerce.guard_product() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.id IS DISTINCT FROM OLD.id OR NEW.sku IS DISTINCT FROM OLD.sku OR NEW.title IS DISTINCT FROM OLD.title OR NEW.unit IS DISTINCT FROM OLD.unit OR NEW.track_stock IS DISTINCT FROM OLD.track_stock OR NEW.currency IS DISTINCT FROM OLD.currency OR NEW.created_at IS DISTINCT FROM OLD.created_at OR NEW.revision<>OLD.revision+1
 THEN RAISE EXCEPTION 'Catalog identity/revision is immutable' USING ERRCODE='23514';END IF;RETURN NEW;
END $$;
CREATE TRIGGER product_guard BEFORE UPDATE ON archava_commerce.products FOR EACH ROW EXECUTE FUNCTION archava_commerce.guard_product();
CREATE FUNCTION archava_commerce.guard_stock_balance() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE total bigint; entries bigint;
BEGIN
 PERFORM id FROM archava_commerce.products WHERE tenant_id=NEW.tenant_id AND id=NEW.product_id FOR UPDATE;
 SELECT coalesce(sum(quantity),0),count(*) INTO total,entries FROM archava_commerce.movements WHERE tenant_id=NEW.tenant_id AND product_id=NEW.product_id;
 IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.product_id IS DISTINCT FROM OLD.product_id OR NEW.quantity IS DISTINCT FROM total OR NEW.revision IS DISTINCT FROM entries+1
 THEN RAISE EXCEPTION 'Stock must derive from immutable movements' USING ERRCODE='23514';END IF;
 IF NEW.quantity<(SELECT coalesce(sum(quantity),0) FROM archava_commerce.allocations WHERE tenant_id=NEW.tenant_id AND product_id=NEW.product_id AND state='held' AND expires_at>clock_timestamp())
 THEN RAISE EXCEPTION 'Stock is already reserved' USING ERRCODE='23514';END IF;RETURN NEW;
END $$;
CREATE TRIGGER stock_balance_guard BEFORE UPDATE ON archava_commerce.stock_balances FOR EACH ROW EXECUTE FUNCTION archava_commerce.guard_stock_balance();
CREATE FUNCTION archava_commerce.apply_movement() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 UPDATE archava_commerce.stock_balances SET quantity=quantity+NEW.quantity,revision=revision+1 WHERE tenant_id=NEW.tenant_id AND product_id=NEW.product_id;
 RETURN NEW;
END $$;
CREATE TRIGGER stock_movement_apply AFTER INSERT ON archava_commerce.movements FOR EACH ROW EXECUTE FUNCTION archava_commerce.apply_movement();
CREATE FUNCTION archava_commerce.guard_movement() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF TG_OP<>'INSERT' THEN RAISE EXCEPTION 'Stock movements are immutable';END IF;
 PERFORM id FROM archava_commerce.products WHERE tenant_id=NEW.tenant_id AND id=NEW.product_id AND track_stock FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Product does not track stock' USING ERRCODE='23514';END IF;
 IF NEW.kind='consume' AND NOT EXISTS(SELECT 1 FROM archava_commerce.allocations a JOIN archava_commerce.orders o ON o.tenant_id=a.tenant_id AND o.id=a.order_id WHERE a.tenant_id=NEW.tenant_id AND a.order_id=NEW.order_id AND a.product_id=NEW.product_id AND a.state='consumed' AND a.quantity=-NEW.quantity AND o.state='reserved')
 THEN RAISE EXCEPTION 'Consumption has no reserved allocation' USING ERRCODE='23514';END IF;
 IF NEW.kind='return' AND NEW.quantity IS DISTINCT FROM (SELECT coalesce(sum(r.quantity),0) FROM archava_commerce.returns r JOIN archava_commerce.order_lines l ON l.tenant_id=r.tenant_id AND l.id=r.line_id WHERE r.tenant_id=NEW.tenant_id AND r.request_id=NEW.request_id AND r.order_id=NEW.order_id AND l.product_id=NEW.product_id AND l.track_stock AND r.disposition='restock')
 THEN RAISE EXCEPTION 'Return has no received line quantity' USING ERRCODE='23514';END IF;RETURN NEW;
END $$;
CREATE TRIGGER movement_guard BEFORE INSERT OR UPDATE OR DELETE ON archava_commerce.movements FOR EACH ROW EXECUTE FUNCTION archava_commerce.guard_movement();
CREATE FUNCTION archava_commerce.guard_allocation() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE observed timestamptz;available bigint;
BEGIN
 PERFORM id FROM archava_commerce.products WHERE tenant_id=NEW.tenant_id AND id=NEW.product_id AND track_stock FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Untracked allocation' USING ERRCODE='23514';END IF;
 observed:=clock_timestamp();
 IF TG_OP='UPDATE' THEN
  IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.order_id IS DISTINCT FROM OLD.order_id OR NEW.product_id IS DISTINCT FROM OLD.product_id OR NEW.quantity IS DISTINCT FROM OLD.quantity OR NEW.expires_at IS DISTINCT FROM OLD.expires_at OR OLD.state<>'held' OR NEW.state NOT IN ('consumed','released') OR (NEW.state='consumed' AND OLD.expires_at<=observed)
  THEN RAISE EXCEPTION 'Allocation transition refused' USING ERRCODE='23514';END IF;
 ELSE
  IF NEW.state<>'held' OR NEW.expires_at<=observed OR NOT EXISTS(SELECT 1 FROM archava_commerce.orders WHERE tenant_id=NEW.tenant_id AND id=NEW.order_id AND state='draft')
  THEN RAISE EXCEPTION 'Allocation requires draft order' USING ERRCODE='23514';END IF;
  SELECT b.quantity-coalesce((SELECT sum(a.quantity) FROM archava_commerce.allocations a WHERE a.tenant_id=NEW.tenant_id AND a.product_id=NEW.product_id AND a.state='held' AND a.expires_at>observed),0) INTO available FROM archava_commerce.stock_balances b WHERE b.tenant_id=NEW.tenant_id AND b.product_id=NEW.product_id;
  IF available<NEW.quantity THEN RAISE EXCEPTION 'Stock allocation exceeds available quantity' USING ERRCODE='23P01';END IF;
 END IF;RETURN NEW;
END $$;
CREATE TRIGGER allocation_guard BEFORE INSERT OR UPDATE ON archava_commerce.allocations FOR EACH ROW EXECUTE FUNCTION archava_commerce.guard_allocation();
CREATE FUNCTION archava_commerce.guard_order() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF TG_OP='INSERT' THEN
  IF NEW.state<>'draft' OR NEW.expires_at IS NOT NULL THEN RAISE EXCEPTION 'Order must begin as draft' USING ERRCODE='23514';END IF;
 ELSE
  IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.id IS DISTINCT FROM OLD.id OR NEW.owner_id IS DISTINCT FROM OLD.owner_id OR NEW.customer_name IS DISTINCT FROM OLD.customer_name OR NEW.managed_by IS DISTINCT FROM OLD.managed_by OR NEW.currency IS DISTINCT FROM OLD.currency OR NEW.created_at IS DISTINCT FROM OLD.created_at OR NEW.revision<>OLD.revision+1
  OR (OLD.state<>'draft' AND (NEW.total_estimate IS DISTINCT FROM OLD.total_estimate OR NEW.expires_at IS DISTINCT FROM OLD.expires_at))
  OR (OLD.fulfilled_at IS NOT NULL AND NEW.fulfilled_at IS DISTINCT FROM OLD.fulfilled_at)
  THEN RAISE EXCEPTION 'Order identity or confirmed quote is immutable' USING ERRCODE='23514';END IF;
  IF NOT ((OLD.state='draft' AND NEW.state IN ('draft','reserved','cancelled')) OR (OLD.state='reserved' AND NEW.state IN ('fulfilled','cancelled')) OR (OLD.state='fulfilled' AND NEW.state IN ('fulfilled','returned')))
  OR (NEW.state='fulfilled' AND OLD.state='reserved' AND OLD.expires_at<=clock_timestamp())
  THEN RAISE EXCEPTION 'Order transition refused' USING ERRCODE='23514';END IF;
 END IF;RETURN NEW;
END $$;
CREATE TRIGGER order_guard BEFORE INSERT OR UPDATE ON archava_commerce.orders FOR EACH ROW EXECUTE FUNCTION archava_commerce.guard_order();
CREATE FUNCTION archava_commerce.guard_line() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE parent text;wanted_tenant text;wanted_order uuid;
BEGIN
 IF TG_OP='DELETE' THEN wanted_tenant:=OLD.tenant_id;wanted_order:=OLD.order_id;ELSE wanted_tenant:=NEW.tenant_id;wanted_order:=NEW.order_id;END IF;
 SELECT state INTO parent FROM archava_commerce.orders WHERE tenant_id=wanted_tenant AND id=wanted_order FOR UPDATE;
 IF TG_OP IN ('INSERT','DELETE') THEN
  IF parent IS DISTINCT FROM 'draft' THEN RAISE EXCEPTION 'Confirmed order lines are immutable' USING ERRCODE='23514';END IF;
  IF TG_OP='DELETE' THEN RETURN OLD;END IF;RETURN NEW;
 END IF;
 IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.id IS DISTINCT FROM OLD.id OR NEW.order_id IS DISTINCT FROM OLD.order_id OR NEW.product_id IS DISTINCT FROM OLD.product_id OR NEW.sku IS DISTINCT FROM OLD.sku OR NEW.title IS DISTINCT FROM OLD.title OR NEW.unit IS DISTINCT FROM OLD.unit OR NEW.track_stock IS DISTINCT FROM OLD.track_stock OR NEW.quantity IS DISTINCT FROM OLD.quantity OR NEW.price IS DISTINCT FROM OLD.price OR NEW.product_revision IS DISTINCT FROM OLD.product_revision OR parent IS DISTINCT FROM 'fulfilled'
 OR NEW.returned_quantity IS DISTINCT FROM (SELECT coalesce(sum(quantity),0) FROM archava_commerce.returns WHERE tenant_id=NEW.tenant_id AND line_id=NEW.id)
 THEN RAISE EXCEPTION 'Line quote or returned quantity is immutable' USING ERRCODE='23514';END IF;RETURN NEW;
END $$;
CREATE TRIGGER line_guard BEFORE INSERT OR UPDATE OR DELETE ON archava_commerce.order_lines FOR EACH ROW EXECUTE FUNCTION archava_commerce.guard_line();
CREATE FUNCTION archava_commerce.guard_return() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE sold integer;received bigint;
BEGIN
 IF TG_OP<>'INSERT' THEN RAISE EXCEPTION 'Returns are immutable';END IF;
 PERFORM id FROM archava_commerce.orders WHERE tenant_id=NEW.tenant_id AND id=NEW.order_id AND state='fulfilled' AND managed_by IS NULL FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Order cannot accept a generic return' USING ERRCODE='23514';END IF;
 SELECT quantity INTO sold FROM archava_commerce.order_lines WHERE tenant_id=NEW.tenant_id AND id=NEW.line_id AND order_id=NEW.order_id;
 SELECT coalesce(sum(quantity),0) INTO received FROM archava_commerce.returns WHERE tenant_id=NEW.tenant_id AND line_id=NEW.line_id;
 IF sold IS NULL OR NEW.quantity+received>sold THEN RAISE EXCEPTION 'Return exceeds delivered quantity' USING ERRCODE='23514';END IF;RETURN NEW;
END $$;
CREATE TRIGGER return_guard BEFORE INSERT OR UPDATE OR DELETE ON archava_commerce.returns FOR EACH ROW EXECUTE FUNCTION archava_commerce.guard_return();
CREATE FUNCTION archava_commerce.apply_return() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN UPDATE archava_commerce.order_lines SET returned_quantity=returned_quantity+NEW.quantity WHERE tenant_id=NEW.tenant_id AND id=NEW.line_id;RETURN NEW;END $$;
CREATE TRIGGER return_apply AFTER INSERT ON archava_commerce.returns FOR EACH ROW EXECUTE FUNCTION archava_commerce.apply_return();
CREATE FUNCTION archava_commerce.assert_order_integrity() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE current_order archava_commerce.orders;lines integer;total bigint;
BEGIN
 SELECT * INTO current_order FROM archava_commerce.orders WHERE tenant_id=NEW.tenant_id AND id=NEW.id;
 SELECT count(*),coalesce(sum(price::bigint*quantity),0) INTO lines,total FROM archava_commerce.order_lines WHERE tenant_id=NEW.tenant_id AND order_id=NEW.id;
 IF lines NOT BETWEEN 1 AND 50 OR total<>current_order.total_estimate THEN RAISE EXCEPTION 'Order quote is incomplete' USING ERRCODE='23514';END IF;
 IF current_order.state='reserved' AND EXISTS(SELECT 1 FROM archava_commerce.allocations WHERE tenant_id=NEW.tenant_id AND order_id=NEW.id AND (state<>'held' OR expires_at IS DISTINCT FROM current_order.expires_at))
 THEN RAISE EXCEPTION 'Reserved order allocation binding refused' USING ERRCODE='23514';END IF;
 IF current_order.state='reserved' AND current_order.managed_by IS NULL AND (
  EXISTS(SELECT 1 FROM archava_commerce.order_lines l WHERE l.tenant_id=NEW.tenant_id AND l.order_id=NEW.id AND l.track_stock AND NOT EXISTS(SELECT 1 FROM archava_commerce.allocations a WHERE a.tenant_id=l.tenant_id AND a.order_id=l.order_id AND a.product_id=l.product_id AND a.quantity=l.quantity))
  OR EXISTS(SELECT 1 FROM archava_commerce.allocations a WHERE a.tenant_id=NEW.tenant_id AND a.order_id=NEW.id AND NOT EXISTS(SELECT 1 FROM archava_commerce.order_lines l WHERE l.tenant_id=a.tenant_id AND l.order_id=a.order_id AND l.product_id=a.product_id AND l.track_stock)))
 THEN RAISE EXCEPTION 'Generic reservation differs from sold quantities' USING ERRCODE='23514';END IF;
 IF current_order.state IN ('fulfilled','returned') AND EXISTS(SELECT 1 FROM archava_commerce.allocations WHERE tenant_id=NEW.tenant_id AND order_id=NEW.id AND state<>'consumed')
 THEN RAISE EXCEPTION 'Delivered order did not consume reserved stock' USING ERRCODE='23514';END IF;
 IF current_order.state='cancelled' AND EXISTS(SELECT 1 FROM archava_commerce.allocations WHERE tenant_id=NEW.tenant_id AND order_id=NEW.id AND state<>'released')
 THEN RAISE EXCEPTION 'Cancelled order did not release stock' USING ERRCODE='23514';END IF;
 IF current_order.state='returned' AND EXISTS(SELECT 1 FROM archava_commerce.order_lines WHERE tenant_id=NEW.tenant_id AND order_id=NEW.id AND returned_quantity<>quantity)
 THEN RAISE EXCEPTION 'Whole order return is incomplete' USING ERRCODE='23514';END IF;RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER complete_order AFTER INSERT OR UPDATE ON archava_commerce.orders DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_commerce.assert_order_integrity();
CREATE FUNCTION archava_commerce.prevent_audit_change() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN RAISE EXCEPTION 'Commerce audit is append-only';END $$;
CREATE TRIGGER audit_immutable BEFORE UPDATE OR DELETE ON archava_commerce.audit FOR EACH ROW EXECUTE FUNCTION archava_commerce.prevent_audit_change();
CREATE FUNCTION archava_commerce.prevent_command_change() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF OLD.receipt IS NOT NULL OR TG_OP='DELETE' OR NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.request_id IS DISTINCT FROM OLD.request_id OR NEW.actor_id IS DISTINCT FROM OLD.actor_id OR NEW.fingerprint IS DISTINCT FROM OLD.fingerprint OR NEW.created_at IS DISTINCT FROM OLD.created_at
 THEN RAISE EXCEPTION 'Commerce command is immutable';END IF;RETURN NEW;
END $$;
CREATE TRIGGER command_immutable BEFORE UPDATE OR DELETE ON archava_commerce.commands FOR EACH ROW EXECUTE FUNCTION archava_commerce.prevent_command_change();
DO $$ DECLARE owned text;BEGIN
 FOREACH owned IN ARRAY ARRAY['memberships','products','stock_balances','orders','order_lines','allocations','commands','movements','returns','audit'] LOOP
  EXECUTE format('ALTER TABLE archava_commerce.%I ENABLE ROW LEVEL SECURITY',owned);
  EXECUTE format('ALTER TABLE archava_commerce.%I FORCE ROW LEVEL SECURITY',owned);
  EXECUTE format('CREATE POLICY commerce_tenant ON archava_commerce.%I USING(tenant_id=current_setting(''archava.tenant_id'',true)) WITH CHECK(tenant_id=current_setting(''archava.tenant_id'',true))',owned);
 END LOOP;
END $$;
CREATE FUNCTION archava_commerce.lock_membership(wanted_tenant text,wanted_principal text) RETURNS SETOF archava_commerce.memberships LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog AS $$
 SELECT * FROM archava_commerce.memberships WHERE tenant_id=wanted_tenant AND principal_id=wanted_principal AND wanted_tenant=current_setting('archava.tenant_id',true) FOR SHARE
$$;
REVOKE ALL ON FUNCTION archava_commerce.lock_membership(text,text) FROM PUBLIC;
