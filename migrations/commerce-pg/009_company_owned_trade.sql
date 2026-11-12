-- Company-owned catalog IDs partition physical balances and FIFO cost layers.
-- Legacy products/documents remain explicitly unassigned; no historical backfill.
ALTER TABLE archava_commerce.products
 ADD COLUMN company_id uuid,ADD COLUMN company_revision integer,ADD COLUMN company_code text,ADD COLUMN company_name text,
 ADD CONSTRAINT product_company_pair CHECK((company_id IS NULL AND company_revision IS NULL AND company_code IS NULL AND company_name IS NULL) OR (company_id IS NOT NULL AND company_revision>0 AND company_revision IS NOT NULL AND company_code IS NOT NULL AND company_name IS NOT NULL)),
 ADD CONSTRAINT product_company_version FOREIGN KEY(tenant_id,company_id,company_revision) REFERENCES archava_organizations.company_audit(tenant_id,company_id,revision);
ALTER TABLE archava_commerce.orders
 ADD COLUMN company_id uuid,ADD COLUMN company_revision integer,ADD COLUMN company_code text,ADD COLUMN company_name text,
 ADD CONSTRAINT sales_company_pair CHECK((company_id IS NULL AND company_revision IS NULL AND company_code IS NULL AND company_name IS NULL) OR (company_id IS NOT NULL AND company_revision>0 AND company_revision IS NOT NULL AND company_code IS NOT NULL AND company_name IS NOT NULL)),
 ADD CONSTRAINT sales_company_version FOREIGN KEY(tenant_id,company_id,company_revision) REFERENCES archava_organizations.company_audit(tenant_id,company_id,revision);
ALTER TABLE archava_commerce.movements
 ADD COLUMN company_id uuid,ADD COLUMN company_revision integer,
 ADD CONSTRAINT movement_company_pair CHECK((company_id IS NULL AND company_revision IS NULL) OR (company_id IS NOT NULL AND company_revision IS NOT NULL AND company_revision>0)),
 ADD CONSTRAINT movement_company_version FOREIGN KEY(tenant_id,company_id,company_revision) REFERENCES archava_organizations.company_audit(tenant_id,company_id,revision);
CREATE INDEX product_company ON archava_commerce.products(tenant_id,company_id,id);
CREATE INDEX sales_company ON archava_commerce.orders(tenant_id,company_id,id);
CREATE FUNCTION archava_commerce.require_company_snapshot(t text,c uuid,r integer,code text,name text) RETURNS void LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE p archava_organizations.companies%ROWTYPE;
BEGIN
 IF c IS NULL THEN RETURN;END IF;
 SELECT * INTO p FROM archava_organizations.companies WHERE tenant_id=t AND id=c FOR SHARE;
 IF NOT FOUND OR NOT p.active OR r IS DISTINCT FROM p.revision OR code IS DISTINCT FROM p.code OR name IS DISTINCT FROM p.legal_name THEN RAISE EXCEPTION 'Company differs from active reviewed profile' USING ERRCODE='23514';END IF;
END $$;
CREATE FUNCTION archava_commerce.guard_company_identity() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF TG_OP='UPDATE' THEN
  IF NEW.company_id IS DISTINCT FROM OLD.company_id OR NEW.company_revision IS DISTINCT FROM OLD.company_revision OR NEW.company_code IS DISTINCT FROM OLD.company_code OR NEW.company_name IS DISTINCT FROM OLD.company_name THEN RAISE EXCEPTION 'Document company identity is immutable' USING ERRCODE='23514';END IF;
  IF TG_TABLE_NAME='products' THEN
   IF NEW.company_id IS NOT NULL AND NEW.active THEN
   PERFORM 1 FROM archava_organizations.companies WHERE tenant_id=NEW.tenant_id AND id=NEW.company_id AND active FOR SHARE;
   IF NOT FOUND THEN RAISE EXCEPTION 'Company is inactive' USING ERRCODE='23514';END IF;
   END IF;
  END IF;
 ELSE
  PERFORM archava_commerce.require_company_snapshot(NEW.tenant_id,NEW.company_id,NEW.company_revision,NEW.company_code,NEW.company_name);
  IF NEW.company_id IS NOT NULL AND NEW.managed_by IS NOT NULL THEN RAISE EXCEPTION 'Managed legacy cohort cannot claim company ownership' USING ERRCODE='23514';END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER a_company_product BEFORE INSERT OR UPDATE ON archava_commerce.products FOR EACH ROW EXECUTE FUNCTION archava_commerce.guard_company_identity();
CREATE TRIGGER a_company_sales BEFORE INSERT OR UPDATE ON archava_commerce.orders FOR EACH ROW EXECUTE FUNCTION archava_commerce.guard_company_identity();
CREATE FUNCTION archava_commerce.guard_line_company() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE owner_company uuid;item_company uuid;
BEGIN
 SELECT company_id INTO owner_company FROM archava_commerce.orders WHERE tenant_id=NEW.tenant_id AND id=NEW.order_id;
 SELECT company_id INTO item_company FROM archava_commerce.products WHERE tenant_id=NEW.tenant_id AND id=NEW.product_id;
 IF owner_company IS DISTINCT FROM item_company THEN RAISE EXCEPTION 'Sales line belongs to another company' USING ERRCODE='23514';END IF;RETURN NEW;
END $$;
CREATE TRIGGER a_company_line BEFORE INSERT OR UPDATE ON archava_commerce.order_lines FOR EACH ROW EXECUTE FUNCTION archava_commerce.guard_line_company();
CREATE FUNCTION archava_commerce.bind_movement_company() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE item_company uuid;source_company uuid;source_revision integer;p archava_organizations.companies%ROWTYPE;
BEGIN
 SELECT company_id INTO item_company FROM archava_commerce.products WHERE tenant_id=NEW.tenant_id AND id=NEW.product_id;
 IF NEW.order_id IS NOT NULL THEN
  SELECT company_id,company_revision INTO source_company,source_revision FROM archava_commerce.orders WHERE tenant_id=NEW.tenant_id AND id=NEW.order_id;
 ELSIF NEW.purchase_id IS NOT NULL THEN
  SELECT (to_jsonb(o)->>'company_id')::uuid,(to_jsonb(o)->>'company_revision')::integer INTO source_company,source_revision FROM archava_purchasing.orders o WHERE tenant_id=NEW.tenant_id AND id=NEW.purchase_id;
 ELSE
  source_company:=NEW.company_id;source_revision:=NEW.company_revision;
  IF source_company IS NOT NULL THEN
   SELECT * INTO p FROM archava_organizations.companies WHERE tenant_id=NEW.tenant_id AND id=source_company FOR SHARE;
   IF NOT FOUND OR NOT p.active OR source_revision IS DISTINCT FROM p.revision THEN RAISE EXCEPTION 'Stock adjustment requires current active company' USING ERRCODE='23514';END IF;
  END IF;
 END IF;
 IF NEW.company_id IS NULL AND NEW.company_revision IS NULL THEN NEW.company_id:=source_company;NEW.company_revision:=source_revision;END IF;
 IF NEW.company_id IS DISTINCT FROM source_company OR NEW.company_revision IS DISTINCT FROM source_revision OR item_company IS DISTINCT FROM source_company THEN RAISE EXCEPTION 'Stock movement belongs to another company' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER a_company_movement BEFORE INSERT ON archava_commerce.movements FOR EACH ROW EXECUTE FUNCTION archava_commerce.bind_movement_company();
