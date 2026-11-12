-- Invoice conversion uses historical order identity, independent of live roles.
ALTER TABLE archava_billing.invoices
 ADD COLUMN customer_party_id uuid,
 ADD COLUMN customer_party_revision integer,
 ADD CONSTRAINT invoice_party_pair CHECK((customer_party_id IS NULL AND customer_party_revision IS NULL) OR (customer_party_id IS NOT NULL AND customer_party_revision IS NOT NULL AND customer_party_revision>0)),
 ADD CONSTRAINT invoice_customer_source CHECK(customer_party_id IS NULL OR customer_contact_id IS NULL),
 ADD CONSTRAINT invoice_party_version FOREIGN KEY(tenant_id,customer_party_id,customer_party_revision) REFERENCES archava_organizations.party_audit(tenant_id,party_id,revision);
CREATE INDEX invoice_party_source ON archava_billing.invoices(tenant_id,customer_party_id);
CREATE FUNCTION archava_billing.guard_customer_party() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE source_id uuid;source_revision integer;
BEGIN
 IF TG_OP='UPDATE' AND (NEW.customer_party_id IS DISTINCT FROM OLD.customer_party_id OR NEW.customer_party_revision IS DISTINCT FROM OLD.customer_party_revision) THEN RAISE EXCEPTION 'Invoice party snapshot is immutable' USING ERRCODE='23514';END IF;
 IF NEW.source_order_id IS NOT NULL THEN
  SELECT customer_party_id,customer_party_revision INTO source_id,source_revision FROM archava_commerce.orders WHERE tenant_id=NEW.tenant_id AND id=NEW.source_order_id;
  IF NOT FOUND OR NEW.customer_party_id IS DISTINCT FROM source_id OR NEW.customer_party_revision IS DISTINCT FROM source_revision THEN RAISE EXCEPTION 'Invoice party differs from historical order' USING ERRCODE='23514';END IF;
 ELSIF NEW.customer_party_id IS NOT NULL THEN
  RAISE EXCEPTION 'Manual invoice cannot claim a sales party source' USING ERRCODE='23514';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER customer_party_guard BEFORE INSERT OR UPDATE ON archava_billing.invoices FOR EACH ROW EXECUTE FUNCTION archava_billing.guard_customer_party();
