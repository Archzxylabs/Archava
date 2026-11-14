-- Tax registration identifiers are fields the company fills, stored as
-- entered. Nothing here knows the format, the checksum or the rules of any
-- tax authority, and nothing is verified.
--
-- A managed company already carries tax_id, the reviewed snapshot of its
-- source organization's tax id (Organizations 004). It gains vat_registered:
-- the company's own statement that it is registered for value-added tax,
-- false on every existing company. A company that states it must have a tax
-- id made of digits and common separators (space, dot, slash, hyphen); that
-- is the only check.
--
-- A business party gains tax_id: the same reviewed snapshot of its source
-- organization's tax id that a company keeps, null on every existing party
-- and always null for a party whose source is a CRM contact (a contact has no
-- tax id). Existing rows are not rewritten.
ALTER TABLE archava_organizations.companies ADD COLUMN vat_registered boolean NOT NULL DEFAULT false;
ALTER TABLE archava_organizations.parties ADD COLUMN tax_id text CHECK(tax_id IS NULL OR length(tax_id) BETWEEN 1 AND 50);
ALTER TABLE archava_organizations.company_audit DROP CONSTRAINT company_audit_operation_check;
ALTER TABLE archava_organizations.company_audit ADD CONSTRAINT company_audit_operation_check CHECK(operation IN ('company.create_from_organization','company.refresh_source','company.set_active','company.set_tax_registration'));

-- guard_company() of 004, replaced in place (same signature, so its trigger is
-- unchanged). Everything it enforced is kept. Added: the tax registration
-- statement changes alone, in a revision of its own; a new company starts
-- without one; and a company that states it has a tax id of digits and
-- separators, also after a refresh from its source.
CREATE OR REPLACE FUNCTION archava_organizations.guard_company() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE s archava_organizations.organizations%ROWTYPE;needs_source boolean;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Company history cannot be deleted' USING ERRCODE='23514';END IF;
 IF TG_OP='UPDATE' THEN
  IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.id IS DISTINCT FROM OLD.id OR NEW.owner_id IS DISTINCT FROM OLD.owner_id OR NEW.code IS DISTINCT FROM OLD.code OR NEW.source_organization_id IS DISTINCT FROM OLD.source_organization_id OR NEW.created_at IS DISTINCT FROM OLD.created_at OR NEW.revision<>OLD.revision+1 THEN RAISE EXCEPTION 'Company identity is immutable' USING ERRCODE='23514';END IF;
  needs_source:=NEW.name IS DISTINCT FROM OLD.name OR NEW.legal_name IS DISTINCT FROM OLD.legal_name OR NEW.tax_id IS DISTINCT FROM OLD.tax_id OR NEW.source_revision IS DISTINCT FROM OLD.source_revision;
  IF needs_source AND (NEW.source_revision<=OLD.source_revision OR NEW.active IS DISTINCT FROM OLD.active) THEN RAISE EXCEPTION 'Company refresh requires an advancing source and unchanged activity' USING ERRCODE='23514';END IF;
  IF NEW.vat_registered IS DISTINCT FROM OLD.vat_registered THEN
   IF needs_source OR NEW.active IS DISTINCT FROM OLD.active THEN RAISE EXCEPTION 'A company tax registration changes in a revision of its own' USING ERRCODE='23514';END IF;
  ELSIF NOT needs_source AND NEW.active=OLD.active THEN RAISE EXCEPTION 'No company change' USING ERRCODE='23514';END IF;
  needs_source:=needs_source OR (NEW.active AND NOT OLD.active);
 ELSE
  IF NEW.revision<>1 OR NOT NEW.active THEN RAISE EXCEPTION 'New company must start active at revision one' USING ERRCODE='23514';END IF;
  IF NEW.vat_registered THEN RAISE EXCEPTION 'A new company starts without a tax registration statement' USING ERRCODE='23514';END IF;
  needs_source:=true;
 END IF;
 IF NEW.vat_registered AND (NEW.tax_id IS NULL OR NEW.tax_id !~ '^[0-9]([0-9 ./-]*[0-9])?$') THEN
  RAISE EXCEPTION 'A company registered for tax needs a tax id of digits and separators' USING ERRCODE='23514';END IF;
 IF needs_source THEN
  SELECT * INTO s FROM archava_organizations.organizations WHERE tenant_id=NEW.tenant_id AND id=NEW.source_organization_id FOR SHARE;
  IF NOT FOUND OR s.state<>'active' OR s.legal_name IS NULL OR btrim(s.legal_name)='' OR NEW.name IS DISTINCT FROM s.name OR NEW.legal_name IS DISTINCT FROM s.legal_name OR NEW.tax_id IS DISTINCT FROM s.tax_id OR NEW.source_revision IS DISTINCT FROM s.revision THEN RAISE EXCEPTION 'Company differs from the reviewed active organization' USING ERRCODE='23514';END IF;
 END IF;
 RETURN NEW;
END $$;

-- guard_party() of 003, replaced in place (same signature, so its trigger is
-- unchanged). The tax id is part of the reviewed source snapshot, like the
-- name: it is the organization's at the reviewed revision, and null for a
-- contact. A refresh still has to advance the source revision, with one
-- exception: a party registered before this migration has no tax id, and may
-- take the one its organization already had at the revision it was reviewed
-- at, with nothing else changed.
CREATE OR REPLACE FUNCTION archava_organizations.guard_party() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE source_name text;source_rev integer;source_state text;source_tax text;needs_source boolean;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Party history cannot be deleted' USING ERRCODE='23514';END IF;
 IF TG_OP='UPDATE' THEN
  IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.id IS DISTINCT FROM OLD.id OR NEW.owner_id IS DISTINCT FROM OLD.owner_id OR NEW.created_at IS DISTINCT FROM OLD.created_at OR NEW.source_contact_id IS DISTINCT FROM OLD.source_contact_id OR NEW.source_organization_id IS DISTINCT FROM OLD.source_organization_id OR NEW.revision<>OLD.revision+1 THEN RAISE EXCEPTION 'Party identity is immutable' USING ERRCODE='23514';END IF;
  needs_source:=NEW.name IS DISTINCT FROM OLD.name OR NEW.source_revision IS DISTINCT FROM OLD.source_revision OR NEW.tax_id IS DISTINCT FROM OLD.tax_id;
  IF needs_source AND NEW.source_revision<=OLD.source_revision
   AND NOT (NEW.source_revision=OLD.source_revision AND NEW.name IS NOT DISTINCT FROM OLD.name AND OLD.tax_id IS NULL AND NEW.tax_id IS NOT NULL
    AND NEW.customer=OLD.customer AND NEW.supplier=OLD.supplier) THEN RAISE EXCEPTION 'Party refresh must advance its source revision' USING ERRCODE='23514';END IF;
  IF NOT needs_source AND NEW.customer=OLD.customer AND NEW.supplier=OLD.supplier THEN RAISE EXCEPTION 'No party change' USING ERRCODE='23514';END IF;
 ELSE
  IF NEW.revision<>1 OR NOT(NEW.customer OR NEW.supplier) THEN RAISE EXCEPTION 'New party requires an active business role' USING ERRCODE='23514';END IF;
  needs_source:=true;
 END IF;
 IF needs_source THEN
  IF NEW.source_contact_id IS NOT NULL THEN
   SELECT full_name,revision,'active',NULL::text INTO source_name,source_rev,source_state,source_tax FROM archava_crm.contacts WHERE tenant_id=NEW.tenant_id AND id=NEW.source_contact_id FOR SHARE;
  ELSE
   SELECT name,revision,state,tax_id INTO source_name,source_rev,source_state,source_tax FROM archava_organizations.organizations WHERE tenant_id=NEW.tenant_id AND id=NEW.source_organization_id FOR SHARE;
  END IF;
  IF NOT FOUND OR source_state<>'active' OR NEW.name IS DISTINCT FROM source_name OR NEW.source_revision IS DISTINCT FROM source_rev OR NEW.tax_id IS DISTINCT FROM source_tax THEN RAISE EXCEPTION 'Party differs from the reviewed current source' USING ERRCODE='23514';END IF;
 END IF;
 RETURN NEW;
END $$;
