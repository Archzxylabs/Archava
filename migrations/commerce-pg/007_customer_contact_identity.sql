-- Explicit CRM identity only. Existing names are never matched or backfilled.
ALTER TABLE archava_commerce.orders
  ADD COLUMN customer_contact_id uuid,
  ADD COLUMN customer_contact_revision integer,
  ADD CONSTRAINT order_customer_contact_fk FOREIGN KEY(tenant_id,customer_contact_id)
    REFERENCES archava_crm.contacts(tenant_id,id),
  ADD CONSTRAINT order_customer_contact_revision CHECK(
    (customer_contact_id IS NULL AND customer_contact_revision IS NULL) OR
    (customer_contact_id IS NOT NULL AND customer_contact_revision IS NOT NULL AND customer_contact_revision>0));
CREATE INDEX orders_customer_contact ON archava_commerce.orders(tenant_id,customer_contact_id,id)
  WHERE customer_contact_id IS NOT NULL;

CREATE FUNCTION archava_commerce.guard_customer_contact() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE customer archava_crm.contacts;
BEGIN
  IF TG_OP='UPDATE' THEN
    IF NEW.customer_contact_id IS DISTINCT FROM OLD.customer_contact_id OR
       NEW.customer_contact_revision IS DISTINCT FROM OLD.customer_contact_revision THEN
      RAISE EXCEPTION 'Order customer identity is immutable';
    END IF;
  ELSIF NEW.customer_contact_id IS NOT NULL THEN
    SELECT * INTO STRICT customer FROM archava_crm.contacts
      WHERE tenant_id=NEW.tenant_id AND id=NEW.customer_contact_id FOR SHARE;
    IF customer.revision IS DISTINCT FROM NEW.customer_contact_revision OR
       customer.full_name IS DISTINCT FROM NEW.customer_name THEN
      RAISE EXCEPTION 'Order customer must match the reviewed CRM snapshot';
    END IF;
  END IF;
  RETURN NEW;
END
$$;
CREATE TRIGGER customer_contact_guard BEFORE INSERT OR UPDATE ON archava_commerce.orders
  FOR EACH ROW EXECUTE FUNCTION archava_commerce.guard_customer_contact();
