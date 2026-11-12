-- A deliberately registered managed company is distinct from a customer/supplier role.
-- Names/tax references are reviewed source snapshots, not legal verification.
ALTER TABLE archava_organizations.memberships DROP CONSTRAINT memberships_permissions_check;
ALTER TABLE archava_organizations.memberships ADD CONSTRAINT memberships_permissions_check CHECK(permissions <@ ARRAY['organizations.read','organizations.write','affiliations.read','affiliations.write','parties.read','parties.write','companies.read','companies.write','audit.read']::text[]);
CREATE TABLE archava_organizations.companies (
 tenant_id text NOT NULL,id uuid NOT NULL,owner_id text NOT NULL,
 code text NOT NULL CHECK(code ~ '^[A-Z0-9][A-Z0-9_-]{0,39}$'),
 source_organization_id uuid NOT NULL,source_revision integer NOT NULL CHECK(source_revision>0),
 name text NOT NULL CHECK(length(name) BETWEEN 1 AND 160),
 legal_name text NOT NULL CHECK(length(legal_name) BETWEEN 1 AND 160 AND btrim(legal_name)<>''),
 tax_id text CHECK(tax_id IS NULL OR length(tax_id) BETWEEN 1 AND 50),
 active boolean NOT NULL DEFAULT true,
 revision integer NOT NULL DEFAULT 1 CHECK(revision>0),created_at timestamptz NOT NULL DEFAULT clock_timestamp(),updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,code),UNIQUE(tenant_id,source_organization_id),
 FOREIGN KEY(tenant_id,owner_id) REFERENCES archava_organizations.memberships(tenant_id,principal_id),
 FOREIGN KEY(tenant_id,source_organization_id) REFERENCES archava_organizations.organizations(tenant_id,id)
);
CREATE INDEX company_owner ON archava_organizations.companies(tenant_id,owner_id,id);
CREATE TABLE archava_organizations.company_audit (
 tenant_id text NOT NULL,request_id uuid NOT NULL,company_id uuid NOT NULL,actor_id text NOT NULL,
 operation text NOT NULL CHECK(operation IN ('company.create_from_organization','company.refresh_source','company.set_active')),
 revision integer NOT NULL CHECK(revision>0),reason text NOT NULL CHECK(length(reason) BETWEEN 1 AND 2000),snapshot jsonb NOT NULL,
 occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),PRIMARY KEY(tenant_id,request_id),UNIQUE(tenant_id,company_id,revision),
 FOREIGN KEY(tenant_id,company_id) REFERENCES archava_organizations.companies(tenant_id,id),
 FOREIGN KEY(tenant_id,request_id) REFERENCES archava_organizations.commands(tenant_id,request_id)
);
CREATE TRIGGER company_audit_immutable BEFORE UPDATE OR DELETE ON archava_organizations.company_audit FOR EACH ROW EXECUTE FUNCTION archava_organizations.immutable_history();
CREATE FUNCTION archava_organizations.guard_company() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE s archava_organizations.organizations%ROWTYPE;needs_source boolean;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Company history cannot be deleted' USING ERRCODE='23514';END IF;
 IF TG_OP='UPDATE' THEN
  IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.id IS DISTINCT FROM OLD.id OR NEW.owner_id IS DISTINCT FROM OLD.owner_id OR NEW.code IS DISTINCT FROM OLD.code OR NEW.source_organization_id IS DISTINCT FROM OLD.source_organization_id OR NEW.created_at IS DISTINCT FROM OLD.created_at OR NEW.revision<>OLD.revision+1 THEN RAISE EXCEPTION 'Company identity is immutable' USING ERRCODE='23514';END IF;
  needs_source:=NEW.name IS DISTINCT FROM OLD.name OR NEW.legal_name IS DISTINCT FROM OLD.legal_name OR NEW.tax_id IS DISTINCT FROM OLD.tax_id OR NEW.source_revision IS DISTINCT FROM OLD.source_revision;
  IF needs_source AND (NEW.source_revision<=OLD.source_revision OR NEW.active IS DISTINCT FROM OLD.active) THEN RAISE EXCEPTION 'Company refresh requires an advancing source and unchanged activity' USING ERRCODE='23514';END IF;
  IF NOT needs_source AND NEW.active=OLD.active THEN RAISE EXCEPTION 'No company change' USING ERRCODE='23514';END IF;
  needs_source:=needs_source OR (NEW.active AND NOT OLD.active);
 ELSE
  IF NEW.revision<>1 OR NOT NEW.active THEN RAISE EXCEPTION 'New company must start active at revision one' USING ERRCODE='23514';END IF;
  needs_source:=true;
 END IF;
 IF needs_source THEN
  SELECT * INTO s FROM archava_organizations.organizations WHERE tenant_id=NEW.tenant_id AND id=NEW.source_organization_id FOR SHARE;
  IF NOT FOUND OR s.state<>'active' OR s.legal_name IS NULL OR btrim(s.legal_name)='' OR NEW.name IS DISTINCT FROM s.name OR NEW.legal_name IS DISTINCT FROM s.legal_name OR NEW.tax_id IS DISTINCT FROM s.tax_id OR NEW.source_revision IS DISTINCT FROM s.revision THEN RAISE EXCEPTION 'Company differs from the reviewed active organization' USING ERRCODE='23514';END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER company_guard BEFORE INSERT OR UPDATE OR DELETE ON archava_organizations.companies FOR EACH ROW EXECUTE FUNCTION archava_organizations.guard_company();
CREATE FUNCTION archava_organizations.assert_company() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE p archava_organizations.companies%ROWTYPE;events integer;last_snapshot jsonb;bound boolean;
BEGIN
 SELECT * INTO p FROM archava_organizations.companies WHERE tenant_id=NEW.tenant_id AND id=NEW.id;
 SELECT count(*) INTO events FROM archava_organizations.company_audit WHERE tenant_id=p.tenant_id AND company_id=p.id;
 SELECT a.snapshot,(c.actor_id=a.actor_id AND c.receipt->>'requestId'=a.request_id::text AND c.receipt->>'companyId'=p.id::text AND c.receipt->>'revision'=p.revision::text AND c.receipt->>'type'=a.operation) INTO last_snapshot,bound FROM archava_organizations.company_audit a JOIN archava_organizations.commands c ON c.tenant_id=a.tenant_id AND c.request_id=a.request_id WHERE a.tenant_id=p.tenant_id AND a.company_id=p.id AND a.revision=p.revision;
 IF events<>p.revision OR last_snapshot IS DISTINCT FROM to_jsonb(p) OR bound IS DISTINCT FROM true THEN RAISE EXCEPTION 'Company audit and receipt are incomplete' USING ERRCODE='23514';END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER company_complete AFTER INSERT OR UPDATE ON archava_organizations.companies DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_organizations.assert_company();
DO $$ DECLARE owned text;BEGIN FOREACH owned IN ARRAY ARRAY['companies','company_audit'] LOOP
 EXECUTE format('ALTER TABLE archava_organizations.%I ENABLE ROW LEVEL SECURITY',owned);
 EXECUTE format('ALTER TABLE archava_organizations.%I FORCE ROW LEVEL SECURITY',owned);
 EXECUTE format('CREATE POLICY company_tenant ON archava_organizations.%I USING(tenant_id=current_setting(''archava.tenant_id'',true)) WITH CHECK(tenant_id=current_setting(''archava.tenant_id'',true))',owned);
END LOOP;END $$;
