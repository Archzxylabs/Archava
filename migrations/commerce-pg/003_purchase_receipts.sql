-- Add inbound provenance without rewriting historical stock or migration checksums.
ALTER TABLE archava_commerce.movements ADD COLUMN purchase_id uuid;
ALTER TABLE archava_commerce.movements DROP CONSTRAINT movements_kind_check;
ALTER TABLE archava_commerce.movements DROP CONSTRAINT movements_check;
ALTER TABLE archava_commerce.movements ADD CONSTRAINT movement_kind CHECK(kind IN ('adjust','consume','return','purchase'));
ALTER TABLE archava_commerce.movements ADD CONSTRAINT movement_source CHECK(
 (kind='adjust' AND order_id IS NULL AND purchase_id IS NULL) OR
 (kind IN ('consume','return') AND order_id IS NOT NULL AND purchase_id IS NULL) OR
 (kind='purchase' AND order_id IS NULL AND purchase_id IS NOT NULL AND quantity>0 AND reason IS NOT NULL)
);
CREATE OR REPLACE FUNCTION archava_commerce.guard_movement() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF TG_OP<>'INSERT' THEN RAISE EXCEPTION 'Stock movements are immutable';END IF;
 PERFORM id FROM archava_commerce.products WHERE tenant_id=NEW.tenant_id AND id=NEW.product_id AND track_stock FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Product does not track stock' USING ERRCODE='23514';END IF;
 IF NEW.kind='consume' AND NOT EXISTS(SELECT 1 FROM archava_commerce.allocations a JOIN archava_commerce.orders o ON o.tenant_id=a.tenant_id AND o.id=a.order_id WHERE a.tenant_id=NEW.tenant_id AND a.order_id=NEW.order_id AND a.product_id=NEW.product_id AND a.state='consumed' AND a.quantity=-NEW.quantity AND o.state='reserved')
 THEN RAISE EXCEPTION 'Consumption has no reserved allocation' USING ERRCODE='23514';END IF;
 IF NEW.kind='return' AND NEW.quantity IS DISTINCT FROM (SELECT coalesce(sum(r.quantity),0) FROM archava_commerce.returns r JOIN archava_commerce.order_lines l ON l.tenant_id=r.tenant_id AND l.id=r.line_id WHERE r.tenant_id=NEW.tenant_id AND r.request_id=NEW.request_id AND r.order_id=NEW.order_id AND l.product_id=NEW.product_id AND l.track_stock AND r.disposition='restock')
 THEN RAISE EXCEPTION 'Return has no received line quantity' USING ERRCODE='23514';END IF;
 IF NEW.kind='purchase' AND NOT EXISTS(SELECT 1 FROM archava_commerce.commands c,jsonb_array_elements(c.receipt->'lines') l WHERE c.tenant_id=NEW.tenant_id AND c.request_id=NEW.request_id AND c.receipt->>'domain'='purchasing' AND (c.receipt->>'id')::uuid=NEW.purchase_id AND (l->>'productId')::uuid=NEW.product_id AND (l->>'quantity')::integer=NEW.quantity)
 THEN RAISE EXCEPTION 'Inbound quantity has no source-bound receipt' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE FUNCTION archava_commerce.assert_purchase_command() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE r jsonb;a archava_commerce.audit;expected integer;actual integer;
BEGIN
 SELECT receipt INTO r FROM archava_commerce.commands WHERE tenant_id=NEW.tenant_id AND request_id=NEW.request_id;
 IF r->>'domain' IS DISTINCT FROM 'purchasing' THEN RETURN NULL;END IF;
 IF r->>'id' IS NULL OR r->>'revision' IS NULL OR jsonb_typeof(r->'lines') IS DISTINCT FROM 'array' THEN RAISE EXCEPTION 'Incomplete inbound receipt' USING ERRCODE='23514';END IF;
 SELECT * INTO a FROM archava_commerce.audit WHERE tenant_id=NEW.tenant_id AND request_id=NEW.request_id;
 IF a.request_id IS NULL OR a.actor_id IS DISTINCT FROM NEW.actor_id OR a.operation IS DISTINCT FROM 'purchase.receive' OR a.record_id IS DISTINCT FROM (r->>'id')::uuid OR a.revision IS DISTINCT FROM (r->>'revision')::integer THEN RAISE EXCEPTION 'Inbound audit binding refused' USING ERRCODE='23514';END IF;
 SELECT count(*) INTO expected FROM jsonb_array_elements(r->'lines');
 SELECT count(*) INTO actual FROM archava_commerce.movements WHERE tenant_id=NEW.tenant_id AND request_id=NEW.request_id;
 IF expected<>actual OR EXISTS(SELECT 1 FROM jsonb_array_elements(r->'lines') l WHERE NOT EXISTS(SELECT 1 FROM archava_commerce.movements m WHERE m.tenant_id=NEW.tenant_id AND m.request_id=NEW.request_id AND m.kind='purchase' AND m.purchase_id=(r->>'id')::uuid AND m.product_id=(l->>'productId')::uuid AND m.quantity=(l->>'quantity')::integer)) THEN RAISE EXCEPTION 'Inbound stock binding refused' USING ERRCODE='23514';END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER complete_purchase_stock AFTER INSERT OR UPDATE ON archava_commerce.commands DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_commerce.assert_purchase_command();
