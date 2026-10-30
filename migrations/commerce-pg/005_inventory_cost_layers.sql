-- Durable acquisition layers share the physical movement transaction.
-- NULL cost is unknown, never zero; lot totals retain indivisible Rupiah.
ALTER TABLE archava_commerce.memberships DROP CONSTRAINT memberships_permissions_check;
ALTER TABLE archava_commerce.memberships ADD CONSTRAINT memberships_permissions_check CHECK(permissions <@ ARRAY['catalog.read','catalog.write','stock.read','stock.write','orders.read','orders.write','returns.write','valuation.read','valuation.write','audit.read']::text[]);
ALTER TABLE archava_commerce.movements ADD COLUMN unit_cost_minor bigint CHECK(unit_cost_minor BETWEEN 0 AND 1000000000);
ALTER TABLE archava_commerce.movements ADD COLUMN landed_fee_minor bigint NOT NULL DEFAULT 0 CHECK(landed_fee_minor BETWEEN 0 AND 500000000000000);

CREATE TABLE archava_commerce.inventory_layers(
 tenant_id text NOT NULL,id bigint GENERATED ALWAYS AS IDENTITY,product_id uuid NOT NULL,
 source_request_id uuid,source_kind text NOT NULL CHECK(source_kind IN('opening','adjust','purchase','return')),
 original_quantity bigint NOT NULL CHECK(original_quantity>0),remaining_quantity bigint NOT NULL CHECK(remaining_quantity BETWEEN 0 AND original_quantity),
 total_cost_minor bigint CHECK(total_cost_minor>=0),created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,id),FOREIGN KEY(tenant_id,product_id) REFERENCES archava_commerce.products(tenant_id,id)
);
CREATE INDEX inventory_fifo ON archava_commerce.inventory_layers(tenant_id,product_id,id) WHERE remaining_quantity>0;
CREATE TABLE archava_commerce.inventory_uses(
 tenant_id text NOT NULL,id bigint GENERATED ALWAYS AS IDENTITY,request_id uuid NOT NULL,product_id uuid NOT NULL,order_id uuid,layer_id bigint NOT NULL,
 start_position bigint NOT NULL CHECK(start_position>=0),quantity bigint NOT NULL CHECK(quantity>0),total_cost_minor bigint CHECK(total_cost_minor>=0),
 returned_quantity bigint NOT NULL DEFAULT 0 CHECK(returned_quantity BETWEEN 0 AND quantity),
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,request_id,product_id,layer_id),
 FOREIGN KEY(tenant_id,layer_id) REFERENCES archava_commerce.inventory_layers(tenant_id,id),
 FOREIGN KEY(tenant_id,order_id) REFERENCES archava_commerce.orders(tenant_id,id)
);
CREATE INDEX inventory_order_cost ON archava_commerce.inventory_uses(tenant_id,order_id,product_id,id);
CREATE TABLE archava_commerce.inventory_return_costs(
 tenant_id text NOT NULL,id bigint GENERATED ALWAYS AS IDENTITY,request_id uuid NOT NULL,line_id uuid NOT NULL,product_id uuid NOT NULL,source_use_id bigint,
 quantity bigint NOT NULL CHECK(quantity>0),total_cost_minor bigint CHECK(total_cost_minor>=0),restock boolean NOT NULL,
 PRIMARY KEY(tenant_id,id),FOREIGN KEY(tenant_id,source_use_id) REFERENCES archava_commerce.inventory_uses(tenant_id,id),
 FOREIGN KEY(tenant_id,request_id,line_id) REFERENCES archava_commerce.returns(tenant_id,request_id,line_id)
);
CREATE INDEX inventory_return_source ON archava_commerce.inventory_return_costs(tenant_id,request_id,product_id);

-- The schema owner briefly bypasses FORCE RLS under this migration's table lock
-- to preserve existing stock as explicitly unvalued opening inventory.
ALTER TABLE archava_commerce.stock_balances NO FORCE ROW LEVEL SECURITY;
INSERT INTO archava_commerce.inventory_layers(tenant_id,product_id,source_kind,original_quantity,remaining_quantity,total_cost_minor)
 SELECT tenant_id,product_id,'opening',quantity,quantity,NULL FROM archava_commerce.stock_balances WHERE quantity>0;
ALTER TABLE archava_commerce.stock_balances FORCE ROW LEVEL SECURITY;

CREATE FUNCTION archava_commerce.cost_portion(total bigint,original bigint,start_at bigint,take_qty bigint) RETURNS bigint LANGUAGE plpgsql IMMUTABLE SET search_path=pg_catalog AS $$
BEGIN
 IF original<=0 OR start_at<0 OR take_qty<=0 OR start_at::numeric+take_qty>original OR total<0 THEN RAISE EXCEPTION 'Invalid cost interval' USING ERRCODE='23514';END IF;
 IF total IS NULL THEN RETURN NULL;END IF;
 RETURN (take_qty::numeric*(total/original)+least(start_at+take_qty,total%original)-least(start_at,total%original))::bigint;
END $$;
CREATE FUNCTION archava_commerce.cost_return() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE product uuid;physical boolean;needed bigint;take_qty bigint;basis bigint;used boolean:=false;u record;
BEGIN
 SELECT product_id,track_stock INTO product,physical FROM archava_commerce.order_lines WHERE tenant_id=NEW.tenant_id AND id=NEW.line_id;
 IF NOT physical THEN RETURN NEW;END IF;
 PERFORM id FROM archava_commerce.products WHERE tenant_id=NEW.tenant_id AND id=product FOR UPDATE;
 needed:=NEW.quantity;
 FOR u IN SELECT x.*,l.original_quantity,l.total_cost_minor AS layer_cost FROM archava_commerce.inventory_uses x JOIN archava_commerce.inventory_layers l ON l.tenant_id=x.tenant_id AND l.id=x.layer_id WHERE x.tenant_id=NEW.tenant_id AND x.order_id=NEW.order_id AND x.product_id=product ORDER BY x.id FOR UPDATE OF x LOOP
  used:=true;take_qty:=least(needed,u.quantity-u.returned_quantity);IF take_qty=0 THEN CONTINUE;END IF;
  basis:=archava_commerce.cost_portion(u.layer_cost,u.original_quantity,u.start_position+u.returned_quantity,take_qty);
  INSERT INTO archava_commerce.inventory_return_costs(tenant_id,request_id,line_id,product_id,source_use_id,quantity,total_cost_minor,restock) VALUES(NEW.tenant_id,NEW.request_id,NEW.line_id,product,u.id,take_qty,basis,NEW.disposition='restock');
  UPDATE archava_commerce.inventory_uses SET returned_quantity=returned_quantity+take_qty WHERE tenant_id=NEW.tenant_id AND id=u.id;
  needed:=needed-take_qty;EXIT WHEN needed=0;
 END LOOP;
 IF needed>0 THEN
  IF used THEN RAISE EXCEPTION 'Return exceeds recorded cost uses' USING ERRCODE='23514';END IF;
  -- A historical pre-costing fulfillment has no known acquisition basis.
  INSERT INTO archava_commerce.inventory_return_costs(tenant_id,request_id,line_id,product_id,quantity,total_cost_minor,restock) VALUES(NEW.tenant_id,NEW.request_id,NEW.line_id,product,needed,NULL,NEW.disposition='restock');
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER zz_inventory_return_cost AFTER INSERT ON archava_commerce.returns FOR EACH ROW EXECUTE FUNCTION archava_commerce.cost_return();
CREATE FUNCTION archava_commerce.cost_movement() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE needed bigint;take_qty bigint;position_at bigint;basis bigint;found_qty bigint;lot record;
BEGIN
 IF NEW.landed_fee_minor<>0 AND NEW.kind<>'purchase' THEN RAISE EXCEPTION 'Landed fee requires a purchase' USING ERRCODE='23514';END IF;
 IF NEW.quantity<0 THEN
  IF NEW.unit_cost_minor IS NOT NULL THEN RAISE EXCEPTION 'Depletion uses its original cost basis' USING ERRCODE='23514';END IF;
  needed:=-NEW.quantity;
  FOR lot IN SELECT * FROM archava_commerce.inventory_layers WHERE tenant_id=NEW.tenant_id AND product_id=NEW.product_id AND remaining_quantity>0 ORDER BY id FOR UPDATE LOOP
   take_qty:=least(needed,lot.remaining_quantity);position_at:=lot.original_quantity-lot.remaining_quantity;
   basis:=archava_commerce.cost_portion(lot.total_cost_minor,lot.original_quantity,position_at,take_qty);
   INSERT INTO archava_commerce.inventory_uses(tenant_id,request_id,product_id,order_id,layer_id,start_position,quantity,total_cost_minor) VALUES(NEW.tenant_id,NEW.request_id,NEW.product_id,NEW.order_id,lot.id,position_at,take_qty,basis);
   UPDATE archava_commerce.inventory_layers SET remaining_quantity=remaining_quantity-take_qty WHERE tenant_id=NEW.tenant_id AND id=lot.id;
   needed:=needed-take_qty;EXIT WHEN needed=0;
  END LOOP;
  IF needed<>0 THEN RAISE EXCEPTION 'Inventory quantity has no matching cost layers' USING ERRCODE='23514';END IF;
 ELSIF NEW.kind='return' THEN
  SELECT coalesce(sum(quantity),0) INTO found_qty FROM archava_commerce.inventory_return_costs WHERE tenant_id=NEW.tenant_id AND request_id=NEW.request_id AND product_id=NEW.product_id AND restock;
  IF found_qty<>NEW.quantity THEN RAISE EXCEPTION 'Restock cost is not source bound' USING ERRCODE='23514';END IF;
  FOR lot IN SELECT * FROM archava_commerce.inventory_return_costs WHERE tenant_id=NEW.tenant_id AND request_id=NEW.request_id AND product_id=NEW.product_id AND restock ORDER BY id LOOP
   INSERT INTO archava_commerce.inventory_layers(tenant_id,product_id,source_request_id,source_kind,original_quantity,remaining_quantity,total_cost_minor) VALUES(NEW.tenant_id,NEW.product_id,NEW.request_id,'return',lot.quantity,lot.quantity,lot.total_cost_minor);
  END LOOP;
 ELSE
  IF NEW.kind='purchase' AND NEW.unit_cost_minor IS NULL THEN RAISE EXCEPTION 'Purchase requires a frozen unit cost' USING ERRCODE='23514';END IF;
  basis:=CASE WHEN NEW.unit_cost_minor IS NULL THEN NULL ELSE NEW.quantity::bigint*NEW.unit_cost_minor+NEW.landed_fee_minor END;
  INSERT INTO archava_commerce.inventory_layers(tenant_id,product_id,source_request_id,source_kind,original_quantity,remaining_quantity,total_cost_minor) VALUES(NEW.tenant_id,NEW.product_id,NEW.request_id,NEW.kind,NEW.quantity,NEW.quantity,basis);
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER zz_inventory_cost_move AFTER INSERT ON archava_commerce.movements FOR EACH ROW EXECUTE FUNCTION archava_commerce.cost_movement();
CREATE FUNCTION archava_commerce.guard_cost_layer() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Acquisition history cannot be deleted' USING ERRCODE='23514';END IF;
 IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.id IS DISTINCT FROM OLD.id OR NEW.product_id IS DISTINCT FROM OLD.product_id OR NEW.source_request_id IS DISTINCT FROM OLD.source_request_id OR NEW.source_kind IS DISTINCT FROM OLD.source_kind OR NEW.original_quantity IS DISTINCT FROM OLD.original_quantity OR NEW.total_cost_minor IS DISTINCT FROM OLD.total_cost_minor OR NEW.created_at IS DISTINCT FROM OLD.created_at OR NEW.remaining_quantity>OLD.remaining_quantity THEN RAISE EXCEPTION 'Acquisition basis is immutable' USING ERRCODE='23514';END IF;RETURN NEW;
END $$;
CREATE TRIGGER cost_layer_guard BEFORE UPDATE OR DELETE ON archava_commerce.inventory_layers FOR EACH ROW EXECUTE FUNCTION archava_commerce.guard_cost_layer();
CREATE FUNCTION archava_commerce.guard_cost_use() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Consumed history cannot be deleted' USING ERRCODE='23514';END IF;
 IF (to_jsonb(NEW)-'returned_quantity') IS DISTINCT FROM (to_jsonb(OLD)-'returned_quantity') OR NEW.returned_quantity<OLD.returned_quantity THEN RAISE EXCEPTION 'Consumed basis is immutable' USING ERRCODE='23514';END IF;RETURN NEW;
END $$;
CREATE TRIGGER cost_use_guard BEFORE UPDATE OR DELETE ON archava_commerce.inventory_uses FOR EACH ROW EXECUTE FUNCTION archava_commerce.guard_cost_use();
CREATE FUNCTION archava_commerce.refuse_cost_history_change() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN RAISE EXCEPTION 'Return cost history is immutable' USING ERRCODE='23514';END $$;
CREATE TRIGGER return_cost_immutable BEFORE UPDATE OR DELETE ON archava_commerce.inventory_return_costs FOR EACH ROW EXECUTE FUNCTION archava_commerce.refuse_cost_history_change();
CREATE FUNCTION archava_commerce.assert_cost_stock() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE physical bigint;layered bigint;
BEGIN
 SELECT quantity INTO physical FROM archava_commerce.stock_balances WHERE tenant_id=NEW.tenant_id AND product_id=NEW.product_id;
 SELECT coalesce(sum(remaining_quantity),0) INTO layered FROM archava_commerce.inventory_layers WHERE tenant_id=NEW.tenant_id AND product_id=NEW.product_id;
 IF physical IS DISTINCT FROM layered THEN RAISE EXCEPTION 'Physical and valued inventory quantities disagree' USING ERRCODE='23514';END IF;RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER inventory_stock_complete AFTER INSERT OR UPDATE ON archava_commerce.stock_balances DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_commerce.assert_cost_stock();
-- Reconcile once per physical movement, rather than once per consumed FIFO lot.
-- Cost tables can only be written by the nested movement/return trigger paths.
CREATE FUNCTION archava_commerce.guard_cost_origin() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF pg_trigger_depth()<2 THEN RAISE EXCEPTION 'Cost history must originate from physical movement or return processing' USING ERRCODE='23514';END IF;
 IF TG_OP='DELETE' THEN RETURN OLD;END IF;RETURN NEW;
END $$;
CREATE TRIGGER cost_layer_origin BEFORE INSERT OR UPDATE OR DELETE ON archava_commerce.inventory_layers FOR EACH ROW EXECUTE FUNCTION archava_commerce.guard_cost_origin();
CREATE TRIGGER cost_use_origin BEFORE INSERT OR UPDATE OR DELETE ON archava_commerce.inventory_uses FOR EACH ROW EXECUTE FUNCTION archava_commerce.guard_cost_origin();
CREATE TRIGGER cost_return_origin BEFORE INSERT OR UPDATE OR DELETE ON archava_commerce.inventory_return_costs FOR EACH ROW EXECUTE FUNCTION archava_commerce.guard_cost_origin();
DO $$ DECLARE owned text;BEGIN
 FOREACH owned IN ARRAY ARRAY['inventory_layers','inventory_uses','inventory_return_costs'] LOOP
  EXECUTE format('ALTER TABLE archava_commerce.%I ENABLE ROW LEVEL SECURITY',owned);
  EXECUTE format('ALTER TABLE archava_commerce.%I FORCE ROW LEVEL SECURITY',owned);
  EXECUTE format('CREATE POLICY inventory_tenant ON archava_commerce.%I USING(tenant_id=current_setting(''archava.tenant_id'',true)) WITH CHECK(tenant_id=current_setting(''archava.tenant_id'',true))',owned);
 END LOOP;
END $$;
