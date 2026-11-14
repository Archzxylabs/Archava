ALTER TABLE archava_purchasing.orders
 ADD COLUMN company_id uuid,ADD COLUMN company_revision integer,ADD COLUMN company_code text,ADD COLUMN company_name text,
 ADD CONSTRAINT purchase_company_pair CHECK((company_id IS NULL AND company_revision IS NULL AND company_code IS NULL AND company_name IS NULL) OR (company_id IS NOT NULL AND company_revision IS NOT NULL AND company_revision>0 AND company_code IS NOT NULL AND company_name IS NOT NULL)),
 ADD CONSTRAINT purchase_company_version FOREIGN KEY(tenant_id,company_id,company_revision) REFERENCES archava_organizations.company_audit(tenant_id,company_id,revision);
CREATE INDEX purchase_company ON archava_purchasing.orders(tenant_id,company_id,id);
CREATE FUNCTION archava_purchasing.guard_company_identity() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF TG_OP='UPDATE' THEN
  IF NEW.company_id IS DISTINCT FROM OLD.company_id OR NEW.company_revision IS DISTINCT FROM OLD.company_revision OR NEW.company_code IS DISTINCT FROM OLD.company_code OR NEW.company_name IS DISTINCT FROM OLD.company_name THEN RAISE EXCEPTION 'Purchase company identity is immutable' USING ERRCODE='23514';END IF;
 ELSE PERFORM archava_commerce.require_company_snapshot(NEW.tenant_id,NEW.company_id,NEW.company_revision,NEW.company_code,NEW.company_name);
 END IF;RETURN NEW;
END $$;
CREATE TRIGGER a_company_order BEFORE INSERT OR UPDATE ON archava_purchasing.orders FOR EACH ROW EXECUTE FUNCTION archava_purchasing.guard_company_identity();
CREATE FUNCTION archava_purchasing.guard_line_company() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE owner_company uuid;item_company uuid;
BEGIN
 SELECT company_id INTO owner_company FROM archava_purchasing.orders WHERE tenant_id=NEW.tenant_id AND id=NEW.order_id;
 SELECT company_id INTO item_company FROM archava_commerce.products WHERE tenant_id=NEW.tenant_id AND id=NEW.product_id;
 IF owner_company IS DISTINCT FROM item_company THEN RAISE EXCEPTION 'Purchase line belongs to another company' USING ERRCODE='23514';END IF;RETURN NEW;
END $$;
CREATE TRIGGER a_company_line BEFORE INSERT OR UPDATE ON archava_purchasing.lines FOR EACH ROW EXECUTE FUNCTION archava_purchasing.guard_line_company();
