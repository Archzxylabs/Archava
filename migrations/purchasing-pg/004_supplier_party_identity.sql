ALTER TABLE archava_purchasing.suppliers
 ADD COLUMN supplier_party_id uuid,
 ADD COLUMN supplier_party_revision integer,
 ADD CONSTRAINT supplier_party_pair CHECK((supplier_party_id IS NULL AND supplier_party_revision IS NULL) OR (supplier_party_id IS NOT NULL AND supplier_party_revision IS NOT NULL AND supplier_party_revision>0)),
 ADD CONSTRAINT supplier_identity_source CHECK(supplier_party_id IS NULL OR supplier_contact_id IS NULL),
 ADD CONSTRAINT supplier_party_version FOREIGN KEY(tenant_id,supplier_party_id,supplier_party_revision) REFERENCES archava_organizations.party_audit(tenant_id,party_id,revision);
ALTER TABLE archava_purchasing.orders
 ADD COLUMN supplier_party_id uuid,
 ADD COLUMN supplier_party_revision integer,
 ADD CONSTRAINT purchase_party_pair CHECK((supplier_party_id IS NULL AND supplier_party_revision IS NULL) OR (supplier_party_id IS NOT NULL AND supplier_party_revision IS NOT NULL AND supplier_party_revision>0)),
 ADD CONSTRAINT purchase_identity_source CHECK(supplier_party_id IS NULL OR supplier_contact_id IS NULL),
 ADD CONSTRAINT purchase_party_version FOREIGN KEY(tenant_id,supplier_party_id,supplier_party_revision) REFERENCES archava_organizations.party_audit(tenant_id,party_id,revision);
CREATE INDEX supplier_party_source ON archava_purchasing.suppliers(tenant_id,supplier_party_id);
CREATE INDEX purchase_party_source ON archava_purchasing.orders(tenant_id,supplier_party_id);
CREATE OR REPLACE FUNCTION archava_purchasing.guard_supplier() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE source_name text;source_revision integer;source_active boolean;needs_source boolean;
BEGIN
 IF TG_OP='UPDATE' THEN
  IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.id IS DISTINCT FROM OLD.id OR NEW.code IS DISTINCT FROM OLD.code OR NEW.created_at IS DISTINCT FROM OLD.created_at OR NEW.supplier_contact_id IS DISTINCT FROM OLD.supplier_contact_id OR NEW.supplier_party_id IS DISTINCT FROM OLD.supplier_party_id OR NEW.revision<>OLD.revision+1 THEN RAISE EXCEPTION 'Supplier identity is immutable' USING ERRCODE='23514';END IF;
  IF NEW.supplier_party_id IS NOT NULL THEN
   needs_source:=NEW.name IS DISTINCT FROM OLD.name OR NEW.supplier_party_revision IS DISTINCT FROM OLD.supplier_party_revision;
   IF needs_source AND (NEW.supplier_party_revision IS NULL OR NEW.supplier_party_revision<=OLD.supplier_party_revision) THEN RAISE EXCEPTION 'Supplier refresh requires advancing party version' USING ERRCODE='23514';END IF;
   needs_source:=needs_source OR (NEW.active AND NOT OLD.active);
   IF NEW.supplier_contact_revision IS DISTINCT FROM OLD.supplier_contact_revision THEN RAISE EXCEPTION 'Supplier source is immutable' USING ERRCODE='23514';END IF;
  ELSE
   IF NEW.supplier_party_revision IS DISTINCT FROM OLD.supplier_party_revision THEN RAISE EXCEPTION 'Supplier source is immutable' USING ERRCODE='23514';END IF;
   needs_source:=NEW.name IS DISTINCT FROM OLD.name OR NEW.supplier_contact_revision IS DISTINCT FROM OLD.supplier_contact_revision;
   IF needs_source AND (NEW.supplier_contact_id IS NULL OR NEW.supplier_contact_revision IS NULL OR NEW.supplier_contact_revision<=OLD.supplier_contact_revision) THEN RAISE EXCEPTION 'Supplier refresh requires advancing CRM snapshot' USING ERRCODE='23514';END IF;
  END IF;
 ELSE
  needs_source:=NEW.supplier_contact_id IS NOT NULL OR NEW.supplier_party_id IS NOT NULL;
 END IF;
 IF needs_source THEN
  IF NEW.supplier_party_id IS NOT NULL THEN
   SELECT name,revision,supplier INTO source_name,source_revision,source_active FROM archava_organizations.parties WHERE tenant_id=NEW.tenant_id AND id=NEW.supplier_party_id FOR SHARE;
   IF NOT FOUND OR NOT source_active OR NEW.name IS DISTINCT FROM source_name OR NEW.supplier_party_revision IS DISTINCT FROM source_revision THEN RAISE EXCEPTION 'Supplier differs from active party master' USING ERRCODE='23514';END IF;
  ELSE
   SELECT full_name,revision INTO source_name,source_revision FROM archava_crm.contacts WHERE tenant_id=NEW.tenant_id AND id=NEW.supplier_contact_id FOR SHARE;
   IF NOT FOUND OR NEW.name IS DISTINCT FROM source_name OR NEW.supplier_contact_revision IS DISTINCT FROM source_revision THEN RAISE EXCEPTION 'Supplier differs from CRM snapshot' USING ERRCODE='23514';END IF;
  END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE FUNCTION archava_purchasing.guard_order_party() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE s archava_purchasing.suppliers%ROWTYPE;p archava_organizations.parties%ROWTYPE;party uuid;
BEGIN
 IF TG_OP='UPDATE' THEN
  IF NEW.supplier_party_id IS DISTINCT FROM OLD.supplier_party_id OR NEW.supplier_party_revision IS DISTINCT FROM OLD.supplier_party_revision THEN RAISE EXCEPTION 'Purchase party snapshot is immutable' USING ERRCODE='23514';END IF;
 ELSE
  -- Source UUID is immutable; peek it before taking locks in party/supplier order.
  SELECT supplier_party_id INTO party FROM archava_purchasing.suppliers WHERE tenant_id=NEW.tenant_id AND id=NEW.supplier_id;
  IF party IS NOT NULL THEN SELECT * INTO p FROM archava_organizations.parties WHERE tenant_id=NEW.tenant_id AND id=party FOR SHARE;END IF;
  SELECT * INTO s FROM archava_purchasing.suppliers WHERE tenant_id=NEW.tenant_id AND id=NEW.supplier_id FOR SHARE;
  IF NEW.supplier_party_id IS NULL AND NEW.supplier_party_revision IS NULL THEN NEW.supplier_party_id:=s.supplier_party_id;NEW.supplier_party_revision:=s.supplier_party_revision;END IF;
  IF NEW.supplier_party_id IS DISTINCT FROM s.supplier_party_id OR NEW.supplier_party_revision IS DISTINCT FROM s.supplier_party_revision OR (party IS NOT NULL AND (NOT p.supplier OR s.supplier_party_revision IS DISTINCT FROM p.revision OR NEW.supplier_name IS DISTINCT FROM s.name OR NEW.supplier_code IS DISTINCT FROM s.code)) THEN RAISE EXCEPTION 'Purchase differs from current supplier/party snapshot' USING ERRCODE='23514';END IF;
 END IF;
 RETURN NEW;
END $$;
-- Sort before the existing supplier/contact locks to preserve lock order.
CREATE TRIGGER a_order_party_guard BEFORE INSERT OR UPDATE ON archava_purchasing.orders FOR EACH ROW EXECUTE FUNCTION archava_purchasing.guard_order_party();
