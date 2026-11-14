-- Preserve the exact original return-cost/use link on each new restock layer.
-- Legacy ambiguous return layers remain NULL; no financial history is inferred.
ALTER TABLE archava_commerce.inventory_layers ADD COLUMN return_cost_id bigint;
ALTER TABLE archava_commerce.inventory_layers ADD CONSTRAINT return_layer_cost_fk FOREIGN KEY(tenant_id,return_cost_id) REFERENCES archava_commerce.inventory_return_costs(tenant_id,id);
CREATE UNIQUE INDEX return_layer_cost_unique ON archava_commerce.inventory_layers(tenant_id,return_cost_id) WHERE return_cost_id IS NOT NULL;
CREATE FUNCTION archava_commerce.guard_return_layer_provenance() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE r archava_commerce.inventory_return_costs;
BEGIN
 IF TG_OP='UPDATE' THEN
  IF NEW.return_cost_id IS DISTINCT FROM OLD.return_cost_id THEN RAISE EXCEPTION 'Return layer provenance is immutable' USING ERRCODE='23514';END IF;
  RETURN NEW;
 END IF;
 IF NEW.source_kind='return' THEN
  SELECT * INTO r FROM archava_commerce.inventory_return_costs WHERE tenant_id=NEW.tenant_id AND id=NEW.return_cost_id;
  IF r.id IS NULL OR NOT r.restock OR (NEW.product_id,NEW.source_request_id,NEW.original_quantity,NEW.remaining_quantity,NEW.total_cost_minor) IS DISTINCT FROM (r.product_id,r.request_id,r.quantity::bigint,r.quantity::bigint,r.total_cost_minor) THEN RAISE EXCEPTION 'Return layer differs from the exact immutable returned use portion' USING ERRCODE='23514';END IF;
 ELSIF NEW.return_cost_id IS NOT NULL THEN
  RAISE EXCEPTION 'Only an actual returned stock layer has a return cost reference' USING ERRCODE='23514';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER return_layer_provenance BEFORE INSERT OR UPDATE ON archava_commerce.inventory_layers FOR EACH ROW EXECUTE FUNCTION archava_commerce.guard_return_layer_provenance();
CREATE OR REPLACE FUNCTION archava_commerce.cost_movement() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog AS $$
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
   INSERT INTO archava_commerce.inventory_layers(tenant_id,product_id,source_request_id,source_kind,original_quantity,remaining_quantity,total_cost_minor,return_cost_id) VALUES(NEW.tenant_id,NEW.product_id,NEW.request_id,'return',lot.quantity,lot.quantity,lot.total_cost_minor,lot.id);
  END LOOP;
 ELSE
  IF NEW.kind='purchase' AND NEW.unit_cost_minor IS NULL THEN RAISE EXCEPTION 'Purchase requires a frozen unit cost' USING ERRCODE='23514';END IF;
  basis:=CASE WHEN NEW.unit_cost_minor IS NULL THEN NULL ELSE NEW.quantity::bigint*NEW.unit_cost_minor+NEW.landed_fee_minor END;
  INSERT INTO archava_commerce.inventory_layers(tenant_id,product_id,source_request_id,source_kind,original_quantity,remaining_quantity,total_cost_minor) VALUES(NEW.tenant_id,NEW.product_id,NEW.request_id,NEW.kind,NEW.quantity,NEW.quantity,basis);
 END IF;
 RETURN NEW;
END $$;

REVOKE ALL ON FUNCTION archava_commerce.cost_movement() FROM PUBLIC;

CREATE FUNCTION archava_commerce.return_layer_guards_complete() RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog AS $$
 SELECT EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid='archava_commerce.inventory_layers'::regclass AND t.tgname='return_layer_provenance'
  AND t.tgfoid='archava_commerce.guard_return_layer_provenance()'::regprocedure AND t.tgtype=23 AND t.tgenabled='O' AND NOT t.tgisinternal
  AND NOT t.tgdeferrable AND NOT t.tginitdeferred AND t.tgqual IS NULL AND t.tgattr=''::int2vector AND encode(t.tgargs,'hex')='')
 AND EXISTS(SELECT 1 FROM pg_constraint c WHERE c.conrelid='archava_commerce.inventory_layers'::regclass AND c.conname='return_layer_cost_fk' AND c.contype='f'
  AND c.confrelid='archava_commerce.inventory_return_costs'::regclass AND c.convalidated AND NOT c.condeferrable AND c.confdeltype='a' AND c.confupdtype='a'
  AND c.conkey=ARRAY[(SELECT attnum FROM pg_attribute WHERE attrelid=c.conrelid AND attname='tenant_id'),(SELECT attnum FROM pg_attribute WHERE attrelid=c.conrelid AND attname='return_cost_id')]::smallint[]
  AND c.confkey=ARRAY[(SELECT attnum FROM pg_attribute WHERE attrelid=c.confrelid AND attname='tenant_id'),(SELECT attnum FROM pg_attribute WHERE attrelid=c.confrelid AND attname='id')]::smallint[])
 AND EXISTS(SELECT 1 FROM pg_index i WHERE i.indexrelid='archava_commerce.return_layer_cost_unique'::regclass AND i.indrelid='archava_commerce.inventory_layers'::regclass
  AND i.indisunique AND i.indisvalid AND i.indnkeyatts=2 AND i.indexprs IS NULL AND pg_get_expr(i.indpred,i.indrelid)='(return_cost_id IS NOT NULL)'
  AND i.indkey=format('%s %s',(SELECT attnum FROM pg_attribute WHERE attrelid=i.indrelid AND attname='tenant_id'),(SELECT attnum FROM pg_attribute WHERE attrelid=i.indrelid AND attname='return_cost_id'))::int2vector)
 AND EXISTS(SELECT 1 FROM pg_proc p WHERE p.oid='archava_commerce.cost_movement()'::regprocedure AND p.prosecdef AND p.proconfig=ARRAY['search_path=pg_catalog'])
 AND NOT has_function_privilege(session_user,'archava_commerce.cost_movement()','EXECUTE')
 AND NOT has_table_privilege(session_user,'archava_commerce.inventory_layers','INSERT,UPDATE,DELETE')
 AND NOT has_table_privilege(session_user,'archava_commerce.inventory_uses','INSERT,UPDATE,DELETE')
 AND NOT has_table_privilege(session_user,'archava_commerce.inventory_return_costs','INSERT,UPDATE,DELETE')
$$;
REVOKE ALL ON FUNCTION archava_commerce.return_layer_guards_complete() FROM PUBLIC;
