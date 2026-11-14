-- Party-linked native sales retain the exact published master version.
ALTER TABLE archava_commerce.orders
 ADD COLUMN customer_party_id uuid,
 ADD COLUMN customer_party_revision integer,
 ADD CONSTRAINT sales_party_pair CHECK((customer_party_id IS NULL AND customer_party_revision IS NULL) OR (customer_party_id IS NOT NULL AND customer_party_revision IS NOT NULL AND customer_party_revision>0)),
 ADD CONSTRAINT sales_customer_source CHECK(customer_party_id IS NULL OR customer_contact_id IS NULL),
 ADD CONSTRAINT sales_party_version FOREIGN KEY(tenant_id,customer_party_id,customer_party_revision) REFERENCES archava_organizations.party_audit(tenant_id,party_id,revision);
CREATE INDEX sales_party_source ON archava_commerce.orders(tenant_id,customer_party_id);
CREATE FUNCTION archava_commerce.guard_customer_party() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE p archava_organizations.parties%ROWTYPE;
BEGIN
 IF TG_OP='UPDATE' THEN
  IF NEW.customer_party_id IS DISTINCT FROM OLD.customer_party_id OR NEW.customer_party_revision IS DISTINCT FROM OLD.customer_party_revision THEN RAISE EXCEPTION 'Sales party snapshot is immutable' USING ERRCODE='23514';END IF;
 ELSIF NEW.customer_party_id IS NOT NULL THEN
  SELECT * INTO p FROM archava_organizations.parties WHERE tenant_id=NEW.tenant_id AND id=NEW.customer_party_id FOR SHARE;
  IF NOT FOUND OR NOT p.customer OR NEW.customer_party_revision IS DISTINCT FROM p.revision OR NEW.customer_name IS DISTINCT FROM p.name OR NEW.managed_by IS NOT NULL OR NEW.customer_contact_id IS NOT NULL THEN RAISE EXCEPTION 'Sales party differs from active reviewed master' USING ERRCODE='23514';END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER customer_party_guard BEFORE INSERT OR UPDATE ON archava_commerce.orders FOR EACH ROW EXECUTE FUNCTION archava_commerce.guard_customer_party();
