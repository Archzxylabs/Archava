ALTER TABLE archava_billing.invoices
 ADD COLUMN company_id uuid,ADD COLUMN company_revision integer,ADD COLUMN company_code text,ADD COLUMN company_name text,
 ADD CONSTRAINT invoice_company_pair CHECK((company_id IS NULL AND company_revision IS NULL AND company_code IS NULL AND company_name IS NULL) OR (company_id IS NOT NULL AND company_revision IS NOT NULL AND company_revision>0 AND company_code IS NOT NULL AND company_name IS NOT NULL)),
 ADD CONSTRAINT invoice_company_version FOREIGN KEY(tenant_id,company_id,company_revision) REFERENCES archava_organizations.company_audit(tenant_id,company_id,revision);
CREATE INDEX invoice_company ON archava_billing.invoices(tenant_id,company_id,id);
CREATE FUNCTION archava_billing.guard_company_identity() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE source archava_commerce.orders%ROWTYPE;
BEGIN
 IF TG_OP='UPDATE' THEN
  IF NEW.company_id IS DISTINCT FROM OLD.company_id OR NEW.company_revision IS DISTINCT FROM OLD.company_revision OR NEW.company_code IS DISTINCT FROM OLD.company_code OR NEW.company_name IS DISTINCT FROM OLD.company_name THEN RAISE EXCEPTION 'Invoice company identity is immutable' USING ERRCODE='23514';END IF;
 ELSIF NEW.source_order_id IS NOT NULL THEN
  SELECT * INTO source FROM archava_commerce.orders WHERE tenant_id=NEW.tenant_id AND id=NEW.source_order_id;
  IF NOT FOUND OR NEW.company_id IS DISTINCT FROM source.company_id OR NEW.company_revision IS DISTINCT FROM source.company_revision OR NEW.company_code IS DISTINCT FROM source.company_code OR NEW.company_name IS DISTINCT FROM source.company_name THEN RAISE EXCEPTION 'Invoice company differs from historical sales order' USING ERRCODE='23514';END IF;
 ELSE PERFORM archava_commerce.require_company_snapshot(NEW.tenant_id,NEW.company_id,NEW.company_revision,NEW.company_code,NEW.company_name);
 END IF;RETURN NEW;
END $$;
CREATE TRIGGER a_company_invoice BEFORE INSERT OR UPDATE ON archava_billing.invoices FOR EACH ROW EXECUTE FUNCTION archava_billing.guard_company_identity();
