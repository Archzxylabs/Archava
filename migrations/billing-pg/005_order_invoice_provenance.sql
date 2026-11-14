-- Existing manual invoices retain NULL provenance; free-text refs are never backfilled.
ALTER TABLE archava_billing.invoices
  DROP CONSTRAINT invoices_customer_ref_check,
  ADD CONSTRAINT invoices_customer_ref_check CHECK(length(customer_ref) BETWEEN 1 AND 160),
  ADD COLUMN source_order_id uuid,
  ADD COLUMN source_order_revision integer,
  ADD CONSTRAINT invoice_source_order_fk FOREIGN KEY(tenant_id,source_order_id)
    REFERENCES archava_commerce.orders(tenant_id,id),
  ADD CONSTRAINT invoice_source_revision CHECK(
    (source_order_id IS NULL AND source_order_revision IS NULL) OR
    (source_order_id IS NOT NULL AND source_order_revision IS NOT NULL AND source_order_revision>0)),
  ADD CONSTRAINT invoice_source_order_unique UNIQUE(tenant_id,source_order_id);

ALTER TABLE archava_billing.invoice_lines
  ADD COLUMN source_order_line_id uuid,
  ADD CONSTRAINT invoice_source_line_fk FOREIGN KEY(tenant_id,source_order_line_id)
    REFERENCES archava_commerce.order_lines(tenant_id,id),
  ADD CONSTRAINT invoice_source_line_unique UNIQUE(tenant_id,source_order_line_id);

CREATE FUNCTION archava_billing.guard_invoice_source() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF NEW.source_order_id IS DISTINCT FROM OLD.source_order_id OR
     NEW.source_order_revision IS DISTINCT FROM OLD.source_order_revision THEN
    RAISE EXCEPTION 'Invoice source identity is immutable';
  END IF;
  RETURN NEW;
END
$$;
CREATE TRIGGER invoice_source_immutable BEFORE UPDATE ON archava_billing.invoices
  FOR EACH ROW EXECUTE FUNCTION archava_billing.guard_invoice_source();

-- Deferred checks see the complete line sheet, within the same tenant transaction.
CREATE FUNCTION archava_billing.assert_order_invoice() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE invoice archava_billing.invoices; source archava_commerce.orders;
  wanted_invoice uuid; expected_lines integer; actual_lines integer;
BEGIN
  IF TG_TABLE_NAME='invoices' THEN wanted_invoice:=NEW.id;
  ELSE wanted_invoice:=NEW.invoice_id;
  END IF;
  SELECT * INTO STRICT invoice FROM archava_billing.invoices
    WHERE tenant_id=NEW.tenant_id AND id=wanted_invoice;
  IF invoice.source_order_id IS NULL THEN
    IF EXISTS(SELECT 1 FROM archava_billing.invoice_lines
      WHERE tenant_id=invoice.tenant_id AND invoice_id=invoice.id AND source_order_line_id IS NOT NULL) THEN
      RAISE EXCEPTION 'Manual invoice cannot claim source lines';
    END IF;
    RETURN NULL;
  END IF;
  SELECT * INTO STRICT source FROM archava_commerce.orders
    WHERE tenant_id=invoice.tenant_id AND id=invoice.source_order_id FOR UPDATE;
  IF source.managed_by IS NOT NULL OR source.state<>'fulfilled' OR
     source.revision<>invoice.source_order_revision OR
     source.customer_name<>invoice.customer_ref OR source.currency<>invoice.currency OR
     source.total_estimate<>invoice.gross_minor OR
     invoice.consumer_ref<>'commerce:'||source.id::text THEN
    RAISE EXCEPTION 'Invoice must match the fulfilled source order snapshot';
  END IF;
  SELECT count(*) INTO expected_lines FROM archava_commerce.order_lines
    WHERE tenant_id=source.tenant_id AND order_id=source.id;
  SELECT count(*) INTO actual_lines FROM archava_billing.invoice_lines
    WHERE tenant_id=invoice.tenant_id AND invoice_id=invoice.id;
  IF expected_lines=0 OR expected_lines<>actual_lines OR EXISTS(
    SELECT 1 FROM archava_billing.invoice_lines line
    LEFT JOIN archava_commerce.order_lines original
      ON original.tenant_id=line.tenant_id AND original.id=line.source_order_line_id
    WHERE line.tenant_id=invoice.tenant_id AND line.invoice_id=invoice.id AND
      (original.id IS NULL OR original.order_id<>source.id OR original.returned_quantity<>0 OR
       original.title<>line.description OR original.quantity<>line.quantity OR
       original.price::bigint<>line.unit_minor OR
       original.quantity::bigint*original.price<>line.line_total_minor)) THEN
    RAISE EXCEPTION 'Invoice lines must exactly match the unreturned source order';
  END IF;
  RETURN NULL;
END
$$;
CREATE CONSTRAINT TRIGGER source_invoice_complete AFTER INSERT ON archava_billing.invoices
  DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_billing.assert_order_invoice();
CREATE CONSTRAINT TRIGGER source_invoice_lines_complete AFTER INSERT ON archava_billing.invoice_lines
  DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_billing.assert_order_invoice();
