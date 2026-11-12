ALTER TABLE archava_billing.invoices
  ADD COLUMN customer_contact_id uuid,
  ADD COLUMN customer_contact_revision integer,
  ADD CONSTRAINT invoice_customer_contact_fk FOREIGN KEY(tenant_id,customer_contact_id)
    REFERENCES archava_crm.contacts(tenant_id,id),
  ADD CONSTRAINT invoice_customer_contact_revision CHECK(
    (customer_contact_id IS NULL AND customer_contact_revision IS NULL) OR
    (customer_contact_id IS NOT NULL AND customer_contact_revision IS NOT NULL AND customer_contact_revision>0));
CREATE INDEX invoices_customer_contact ON archava_billing.invoices(tenant_id,customer_contact_id,id)
  WHERE customer_contact_id IS NOT NULL;

CREATE FUNCTION archava_billing.guard_customer_contact() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE source_contact uuid; source_revision integer;
BEGIN
  IF TG_OP='UPDATE' THEN
    IF NEW.customer_contact_id IS DISTINCT FROM OLD.customer_contact_id OR
       NEW.customer_contact_revision IS DISTINCT FROM OLD.customer_contact_revision THEN
      RAISE EXCEPTION 'Invoice customer identity is immutable';
    END IF;
  ELSIF NEW.source_order_id IS NOT NULL THEN
    -- Preserve the historical source snapshot even if CRM has since changed.
    SELECT customer_contact_id,customer_contact_revision INTO STRICT source_contact,source_revision
      FROM archava_commerce.orders WHERE tenant_id=NEW.tenant_id AND id=NEW.source_order_id;
    IF NEW.customer_contact_id IS DISTINCT FROM source_contact OR
       NEW.customer_contact_revision IS DISTINCT FROM source_revision THEN
      RAISE EXCEPTION 'Invoice customer identity must match its source order';
    END IF;
  ELSIF NEW.customer_contact_id IS NOT NULL THEN
    RAISE EXCEPTION 'Linked invoice customer requires a verified source order';
  END IF;
  RETURN NEW;
END
$$;
CREATE TRIGGER invoice_customer_contact_guard BEFORE INSERT OR UPDATE ON archava_billing.invoices
  FOR EACH ROW EXECUTE FUNCTION archava_billing.guard_customer_contact();
