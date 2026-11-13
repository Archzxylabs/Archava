-- Keep pre-migration affiliations explicitly unverified; never infer a contact
-- from its name or silently bless old UUID references.
ALTER TABLE archava_organizations.affiliations
 ADD COLUMN contact_revision integer,
 ADD COLUMN contact_name text,
 ADD CONSTRAINT affiliation_contact_snapshot_pair CHECK (
  (contact_revision IS NULL AND contact_name IS NULL) OR
  (contact_revision IS NOT NULL AND contact_revision>0 AND contact_name IS NOT NULL AND length(contact_name) BETWEEN 1 AND 160)),
 ADD CONSTRAINT affiliation_contact_fk FOREIGN KEY(tenant_id,contact_id)
  REFERENCES archava_crm.contacts(tenant_id,id) NOT VALID;
-- NOT VALID preserves unknown historical references while enforcing the FK on
-- every new reference. An explicit reviewed command verifies an old snapshot.
CREATE FUNCTION archava_organizations.guard_affiliation_contact() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE current_name text;current_revision integer;needs_source boolean;
BEGIN
 IF TG_OP='UPDATE' THEN
  IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.id IS DISTINCT FROM OLD.id
   OR NEW.organization_id IS DISTINCT FROM OLD.organization_id
   OR NEW.contact_id IS DISTINCT FROM OLD.contact_id
   OR NEW.created_at IS DISTINCT FROM OLD.created_at THEN
   RAISE EXCEPTION 'Affiliation identity is immutable' USING ERRCODE='23514';
  END IF;
  needs_source:=NEW.contact_revision IS DISTINCT FROM OLD.contact_revision OR NEW.contact_name IS DISTINCT FROM OLD.contact_name;
  IF needs_source AND (OLD.contact_revision IS NOT NULL OR NEW.contact_revision IS NULL OR NEW.contact_name IS NULL) THEN
   RAISE EXCEPTION 'Verified affiliation snapshot is immutable' USING ERRCODE='23514';
  END IF;
  IF NEW.contact_revision IS NULL AND ((NEW.status='active' AND OLD.status<>'active') OR (NEW.is_primary AND NOT OLD.is_primary)) THEN
   RAISE EXCEPTION 'Verify the legacy CRM source before activating or promoting it' USING ERRCODE='23514';
  END IF;
 ELSE
  needs_source:=true;
 END IF;
 IF needs_source THEN
  SELECT full_name,revision INTO current_name,current_revision FROM archava_crm.contacts
   WHERE tenant_id=NEW.tenant_id AND id=NEW.contact_id FOR SHARE;
  IF NOT FOUND THEN RAISE EXCEPTION 'Affiliation CRM contact missing' USING ERRCODE='23514';END IF;
  IF TG_OP='INSERT' AND NEW.contact_revision IS NULL AND NEW.contact_name IS NULL THEN
   NEW.contact_revision:=current_revision;NEW.contact_name:=current_name;
  END IF;
  IF NEW.contact_revision IS DISTINCT FROM current_revision OR NEW.contact_name IS DISTINCT FROM current_name THEN
   RAISE EXCEPTION 'Affiliation CRM snapshot differs from current source' USING ERRCODE='23514';
  END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER affiliation_contact_guard BEFORE INSERT OR UPDATE ON archava_organizations.affiliations
 FOR EACH ROW EXECUTE FUNCTION archava_organizations.guard_affiliation_contact();
