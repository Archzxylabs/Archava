-- Freeze delivery costs on the accepted portion of this receipt, never rejects.
ALTER TABLE archava_purchasing.receipts ADD COLUMN landed_fee_minor bigint NOT NULL DEFAULT 0 CHECK(landed_fee_minor BETWEEN 0 AND 500000000000000);
ALTER TABLE archava_purchasing.receipts ADD COLUMN allocation_basis text NOT NULL DEFAULT 'value' CHECK(allocation_basis IN('value','quantity'));
CREATE FUNCTION archava_purchasing.assert_received_cost() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE bad integer;basis_total numeric;received_count integer;moved_count integer;
BEGIN
 SELECT count(*),coalesce(sum(CASE WHEN NEW.allocation_basis='value' THEN r.accepted::numeric*l.unit_cost ELSE r.accepted::numeric END),0) INTO received_count,basis_total FROM archava_purchasing.receipt_lines r JOIN archava_purchasing.lines l ON l.tenant_id=r.tenant_id AND l.id=r.line_id WHERE r.tenant_id=NEW.tenant_id AND r.request_id=NEW.request_id AND r.accepted>0;
 IF NEW.landed_fee_minor>0 AND basis_total=0 THEN RAISE EXCEPTION 'Landed fee has no positive accepted allocation basis' USING ERRCODE='23514';END IF;
 SELECT count(*) INTO moved_count FROM archava_commerce.movements WHERE tenant_id=NEW.tenant_id AND request_id=NEW.request_id AND kind='purchase' AND purchase_id=NEW.order_id;
 IF moved_count<>received_count THEN RAISE EXCEPTION 'Received costs require every accepted stock movement' USING ERRCODE='23514';END IF;
 WITH basis AS(
  SELECT r.line_id,r.product_id,r.accepted,l.unit_cost,CASE WHEN NEW.allocation_basis='value' THEN r.accepted::numeric*l.unit_cost ELSE r.accepted::numeric END AS weight
  FROM archava_purchasing.receipt_lines r JOIN archava_purchasing.lines l ON l.tenant_id=r.tenant_id AND l.id=r.line_id WHERE r.tenant_id=NEW.tenant_id AND r.request_id=NEW.request_id AND r.accepted>0
 ),quotas AS(
  SELECT *,CASE WHEN basis_total=0 THEN 0 ELSE trunc(NEW.landed_fee_minor::numeric*weight/basis_total) END AS floor_fee,CASE WHEN basis_total=0 THEN 0 ELSE mod(NEW.landed_fee_minor::numeric*weight,basis_total) END AS remainder FROM basis
 ),ranked AS(
  SELECT *,row_number() OVER(ORDER BY remainder DESC,line_id) AS position,sum(floor_fee) OVER() AS assigned FROM quotas
 )
 SELECT count(*) INTO bad FROM ranked r LEFT JOIN archava_commerce.movements m ON m.tenant_id=NEW.tenant_id AND m.request_id=NEW.request_id AND m.product_id=r.product_id AND m.kind='purchase'
 WHERE m.product_id IS NULL OR m.quantity<>r.accepted OR m.unit_cost_minor IS DISTINCT FROM r.unit_cost::bigint OR m.landed_fee_minor IS DISTINCT FROM (r.floor_fee+CASE WHEN r.position<=NEW.landed_fee_minor-r.assigned THEN 1 ELSE 0 END)::bigint;
 IF bad<>0 THEN RAISE EXCEPTION 'Received acquisition costs differ from the frozen receipt allocation' USING ERRCODE='23514';END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER received_cost_complete AFTER INSERT ON archava_purchasing.receipts DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_purchasing.assert_received_cost();
