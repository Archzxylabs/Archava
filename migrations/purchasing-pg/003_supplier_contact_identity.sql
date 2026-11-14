-- Explicit CRM source for suppliers. Historical purchase snapshots never refresh.
ALTER TABLE archava_purchasing.suppliers
 ADD COLUMN supplier_contact_id uuid,
 ADD COLUMN supplier_contact_revision integer,
 ADD CONSTRAINT supplier_contact_pair CHECK (
  (supplier_contact_id IS NULL AND supplier_contact_revision IS NULL) OR
  (supplier_contact_id IS NOT NULL AND supplier_contact_revision IS NOT NULL AND supplier_contact_revision > 0)),
 ADD CONSTRAINT supplier_contact_fk FOREIGN KEY(tenant_id,supplier_contact_id)
  REFERENCES archava_crm.contacts(tenant_id,id);
ALTER TABLE archava_purchasing.orders
 ADD COLUMN supplier_contact_id uuid,
 ADD COLUMN supplier_contact_revision integer,
 ADD CONSTRAINT purchase_contact_pair CHECK (
  (supplier_contact_id IS NULL AND supplier_contact_revision IS NULL) OR
  (supplier_contact_id IS NOT NULL AND supplier_contact_revision IS NOT NULL AND supplier_contact_revision > 0)),
 ADD CONSTRAINT purchase_contact_fk FOREIGN KEY(tenant_id,supplier_contact_id)
  REFERENCES archava_crm.contacts(tenant_id,id);
CREATE INDEX supplier_contact_source ON archava_purchasing.suppliers(tenant_id,supplier_contact_id);
CREATE INDEX purchase_contact_source ON archava_purchasing.orders(tenant_id,supplier_contact_id);

CREATE OR REPLACE FUNCTION archava_purchasing.guard_supplier() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE source_name text;source_revision integer;needs_source boolean;
BEGIN
 IF TG_OP='UPDATE' THEN
  IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.id IS DISTINCT FROM OLD.id
   OR NEW.code IS DISTINCT FROM OLD.code OR NEW.created_at IS DISTINCT FROM OLD.created_at
   OR NEW.supplier_contact_id IS DISTINCT FROM OLD.supplier_contact_id
   OR NEW.revision<>OLD.revision+1 THEN
   RAISE EXCEPTION 'Supplier identity is immutable' USING ERRCODE='23514';
  END IF;
  needs_source:=NEW.name IS DISTINCT FROM OLD.name OR NEW.supplier_contact_revision IS DISTINCT FROM OLD.supplier_contact_revision;
  IF needs_source AND (NEW.supplier_contact_id IS NULL OR NEW.supplier_contact_revision IS NULL
    OR NEW.supplier_contact_revision<=OLD.supplier_contact_revision) THEN
   RAISE EXCEPTION 'Supplier refresh requires an advancing CRM snapshot' USING ERRCODE='23514';
  END IF;
 ELSE
  needs_source:=NEW.supplier_contact_id IS NOT NULL;
 END IF;
 IF needs_source THEN
  SELECT full_name,revision INTO source_name,source_revision FROM archava_crm.contacts
   WHERE tenant_id=NEW.tenant_id AND id=NEW.supplier_contact_id FOR SHARE;
  IF NOT FOUND OR NEW.name IS DISTINCT FROM source_name OR NEW.supplier_contact_revision IS DISTINCT FROM source_revision THEN
   RAISE EXCEPTION 'Supplier CRM snapshot differs from current source' USING ERRCODE='23514';
  END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER supplier_contact_insert BEFORE INSERT ON archava_purchasing.suppliers
 FOR EACH ROW EXECUTE FUNCTION archava_purchasing.guard_supplier();

CREATE FUNCTION archava_purchasing.guard_order_contact() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE source archava_purchasing.suppliers%ROWTYPE;
BEGIN
 IF TG_OP='UPDATE' THEN
  IF NEW.supplier_contact_id IS DISTINCT FROM OLD.supplier_contact_id
   OR NEW.supplier_contact_revision IS DISTINCT FROM OLD.supplier_contact_revision THEN
   RAISE EXCEPTION 'Purchase CRM snapshot is immutable' USING ERRCODE='23514';
  END IF;
 ELSE
  SELECT * INTO source FROM archava_purchasing.suppliers
   WHERE tenant_id=NEW.tenant_id AND id=NEW.supplier_id FOR SHARE;
  IF NOT FOUND THEN RAISE EXCEPTION 'Purchase supplier missing' USING ERRCODE='23514';END IF;
  -- Existing callers omit the new fields. Derive from the chosen supplier UUID,
  -- never by matching a name. Do not backfill purchases created before this SQL.
  IF NEW.supplier_contact_id IS NULL AND NEW.supplier_contact_revision IS NULL THEN
   NEW.supplier_contact_id:=source.supplier_contact_id;
   NEW.supplier_contact_revision:=source.supplier_contact_revision;
  END IF;
  IF NEW.supplier_contact_id IS DISTINCT FROM source.supplier_contact_id
   OR NEW.supplier_contact_revision IS DISTINCT FROM source.supplier_contact_revision
   OR (source.supplier_contact_id IS NOT NULL AND
     (NEW.supplier_name IS DISTINCT FROM source.name OR NEW.supplier_code IS DISTINCT FROM source.code)) THEN
   RAISE EXCEPTION 'Purchase supplier snapshot differs from selected supplier' USING ERRCODE='23514';
  END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER order_contact_guard BEFORE INSERT OR UPDATE ON archava_purchasing.orders
 FOR EACH ROW EXECUTE FUNCTION archava_purchasing.guard_order_contact();
