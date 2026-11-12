-- Business roles belong to a stable, explicitly reviewed source identity.
ALTER TABLE archava_organizations.memberships DROP CONSTRAINT memberships_permissions_check;
ALTER TABLE archava_organizations.memberships ADD CONSTRAINT memberships_permissions_check CHECK(permissions <@ ARRAY['organizations.read','organizations.write','affiliations.read','affiliations.write','parties.read','parties.write','audit.read']::text[]);
CREATE TABLE archava_organizations.parties (
 tenant_id text NOT NULL,id uuid NOT NULL,owner_id text NOT NULL,
 source_contact_id uuid,source_organization_id uuid,source_revision integer NOT NULL CHECK(source_revision>0),
 name text NOT NULL CHECK(length(name) BETWEEN 1 AND 160),customer boolean NOT NULL,supplier boolean NOT NULL,
 revision integer NOT NULL DEFAULT 1 CHECK(revision>0),created_at timestamptz NOT NULL DEFAULT clock_timestamp(),updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,id),
 CHECK((source_contact_id IS NOT NULL)::integer+(source_organization_id IS NOT NULL)::integer=1),
 FOREIGN KEY(tenant_id,owner_id) REFERENCES archava_organizations.memberships(tenant_id,principal_id),
 FOREIGN KEY(tenant_id,source_contact_id) REFERENCES archava_crm.contacts(tenant_id,id),
 FOREIGN KEY(tenant_id,source_organization_id) REFERENCES archava_organizations.organizations(tenant_id,id)
);
CREATE UNIQUE INDEX party_contact_identity ON archava_organizations.parties(tenant_id,source_contact_id) WHERE source_contact_id IS NOT NULL;
CREATE UNIQUE INDEX party_organization_identity ON archava_organizations.parties(tenant_id,source_organization_id) WHERE source_organization_id IS NOT NULL;
CREATE INDEX party_owner ON archava_organizations.parties(tenant_id,owner_id,id);
CREATE TABLE archava_organizations.party_audit (
 tenant_id text NOT NULL,request_id uuid NOT NULL,party_id uuid NOT NULL,actor_id text NOT NULL,operation text NOT NULL,
 revision integer NOT NULL CHECK(revision>0),reason text CHECK(reason IS NULL OR length(reason) BETWEEN 1 AND 2000),snapshot jsonb NOT NULL,
 occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),PRIMARY KEY(tenant_id,request_id),UNIQUE(tenant_id,party_id,revision),
 FOREIGN KEY(tenant_id,party_id) REFERENCES archava_organizations.parties(tenant_id,id),
 FOREIGN KEY(tenant_id,request_id) REFERENCES archava_organizations.commands(tenant_id,request_id)
);
CREATE TRIGGER party_audit_immutable BEFORE UPDATE OR DELETE ON archava_organizations.party_audit FOR EACH ROW EXECUTE FUNCTION archava_organizations.immutable_history();
CREATE FUNCTION archava_organizations.guard_party() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE source_name text;source_rev integer;source_state text;needs_source boolean;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Party history cannot be deleted' USING ERRCODE='23514';END IF;
 IF TG_OP='UPDATE' THEN
  IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.id IS DISTINCT FROM OLD.id OR NEW.owner_id IS DISTINCT FROM OLD.owner_id OR NEW.created_at IS DISTINCT FROM OLD.created_at OR NEW.source_contact_id IS DISTINCT FROM OLD.source_contact_id OR NEW.source_organization_id IS DISTINCT FROM OLD.source_organization_id OR NEW.revision<>OLD.revision+1 THEN RAISE EXCEPTION 'Party identity is immutable' USING ERRCODE='23514';END IF;
  needs_source:=NEW.name IS DISTINCT FROM OLD.name OR NEW.source_revision IS DISTINCT FROM OLD.source_revision;
  IF needs_source AND NEW.source_revision<=OLD.source_revision THEN RAISE EXCEPTION 'Party refresh must advance its source revision' USING ERRCODE='23514';END IF;
  IF NOT needs_source AND NEW.customer=OLD.customer AND NEW.supplier=OLD.supplier THEN RAISE EXCEPTION 'No party change' USING ERRCODE='23514';END IF;
 ELSE
  IF NEW.revision<>1 OR NOT(NEW.customer OR NEW.supplier) THEN RAISE EXCEPTION 'New party requires an active business role' USING ERRCODE='23514';END IF;
  needs_source:=true;
 END IF;
 IF needs_source THEN
  IF NEW.source_contact_id IS NOT NULL THEN
   SELECT full_name,revision,'active' INTO source_name,source_rev,source_state FROM archava_crm.contacts WHERE tenant_id=NEW.tenant_id AND id=NEW.source_contact_id FOR SHARE;
  ELSE
   SELECT name,revision,state INTO source_name,source_rev,source_state FROM archava_organizations.organizations WHERE tenant_id=NEW.tenant_id AND id=NEW.source_organization_id FOR SHARE;
  END IF;
  IF NOT FOUND OR source_state<>'active' OR NEW.name IS DISTINCT FROM source_name OR NEW.source_revision IS DISTINCT FROM source_rev THEN RAISE EXCEPTION 'Party differs from the reviewed current source' USING ERRCODE='23514';END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER party_guard BEFORE INSERT OR UPDATE OR DELETE ON archava_organizations.parties FOR EACH ROW EXECUTE FUNCTION archava_organizations.guard_party();
CREATE FUNCTION archava_organizations.assert_party() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE p archava_organizations.parties%ROWTYPE;events integer;last_snapshot jsonb;bound boolean;
BEGIN
 SELECT * INTO p FROM archava_organizations.parties WHERE tenant_id=NEW.tenant_id AND id=NEW.id;
 SELECT count(*) INTO events FROM archava_organizations.party_audit WHERE tenant_id=p.tenant_id AND party_id=p.id;
 SELECT a.snapshot,(c.actor_id=a.actor_id AND c.receipt->>'requestId'=a.request_id::text AND c.receipt->>'partyId'=p.id::text AND c.receipt->>'revision'=p.revision::text AND c.receipt->>'type'=a.operation) INTO last_snapshot,bound FROM archava_organizations.party_audit a JOIN archava_organizations.commands c ON c.tenant_id=a.tenant_id AND c.request_id=a.request_id WHERE a.tenant_id=p.tenant_id AND a.party_id=p.id AND a.revision=p.revision;
 IF events<>p.revision OR last_snapshot IS DISTINCT FROM to_jsonb(p) OR bound IS DISTINCT FROM true THEN RAISE EXCEPTION 'Party audit and receipt are incomplete' USING ERRCODE='23514';END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER party_complete AFTER INSERT OR UPDATE ON archava_organizations.parties DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_organizations.assert_party();
DO $$ DECLARE owned text;BEGIN FOREACH owned IN ARRAY ARRAY['parties','party_audit'] LOOP
 EXECUTE format('ALTER TABLE archava_organizations.%I ENABLE ROW LEVEL SECURITY',owned);
 EXECUTE format('ALTER TABLE archava_organizations.%I FORCE ROW LEVEL SECURITY',owned);
 EXECUTE format('CREATE POLICY party_tenant ON archava_organizations.%I USING(tenant_id=current_setting(''archava.tenant_id'',true)) WITH CHECK(tenant_id=current_setting(''archava.tenant_id'',true))',owned);
END LOOP;END $$;
