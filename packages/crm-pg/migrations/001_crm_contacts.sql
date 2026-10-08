CREATE TABLE archava_crm.memberships (
  tenant_id text NOT NULL,
  principal_id text NOT NULL,
  kind text NOT NULL CHECK (kind IN ('human','service')),
  permissions text[] NOT NULL CHECK (permissions <@ ARRAY['contacts.read','contacts.write','activities.read','activities.write','followups.read','followups.write','audit.read']::text[]),
  scope text NOT NULL CHECK (scope IN ('all','assigned')),
  active boolean NOT NULL DEFAULT true,
  PRIMARY KEY (tenant_id, principal_id)
);
CREATE TABLE archava_crm.contacts (
  tenant_id text NOT NULL, id uuid NOT NULL,
  full_name text NOT NULL CHECK (length(full_name) BETWEEN 1 AND 160),
  email text CHECK (length(email) <= 254), organization text CHECK (length(organization) <= 160),
  owner_id text NOT NULL, revision integer NOT NULL DEFAULT 1 CHECK (revision > 0),
  created_at timestamptz NOT NULL DEFAULT now(), updated_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (tenant_id,id),
  FOREIGN KEY (tenant_id,owner_id) REFERENCES archava_crm.memberships(tenant_id,principal_id)
);
CREATE TABLE archava_crm.activities (
  tenant_id text NOT NULL, id uuid NOT NULL, contact_id uuid NOT NULL,
  kind text NOT NULL CHECK (kind IN ('note','call','meeting')),
  summary text NOT NULL CHECK (length(summary) BETWEEN 1 AND 2000), actor_id text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(), PRIMARY KEY (tenant_id,id),
  FOREIGN KEY (tenant_id,contact_id) REFERENCES archava_crm.contacts(tenant_id,id),
  FOREIGN KEY (tenant_id,actor_id) REFERENCES archava_crm.memberships(tenant_id,principal_id)
);
CREATE TABLE archava_crm.followups (
  tenant_id text NOT NULL, id uuid NOT NULL, contact_id uuid NOT NULL,
  summary text NOT NULL CHECK (length(summary) BETWEEN 1 AND 2000), due_at timestamptz NOT NULL,
  status text NOT NULL DEFAULT 'open' CHECK (status IN ('open','completed')),
  revision integer NOT NULL DEFAULT 1 CHECK (revision > 0),
  created_at timestamptz NOT NULL DEFAULT now(), updated_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (tenant_id,id),
  FOREIGN KEY (tenant_id,contact_id) REFERENCES archava_crm.contacts(tenant_id,id)
);
CREATE INDEX crm_activities_contact ON archava_crm.activities(tenant_id,contact_id,id);
CREATE INDEX crm_followups_contact ON archava_crm.followups(tenant_id,contact_id,id);
CREATE INDEX crm_contacts_owner ON archava_crm.contacts(tenant_id,owner_id,id);
CREATE TABLE archava_crm.commands (
  tenant_id text NOT NULL, request_id uuid NOT NULL, actor_id text NOT NULL,
  fingerprint text NOT NULL, receipt jsonb,
  created_at timestamptz NOT NULL DEFAULT now(), PRIMARY KEY (tenant_id,request_id),
  FOREIGN KEY (tenant_id,actor_id) REFERENCES archava_crm.memberships(tenant_id,principal_id)
);
CREATE TABLE archava_crm.audit (
  tenant_id text NOT NULL, request_id uuid NOT NULL, actor_id text NOT NULL,
  operation text NOT NULL, resource_id uuid NOT NULL, revision integer NOT NULL,
  occurred_at timestamptz NOT NULL DEFAULT now(), PRIMARY KEY (tenant_id,request_id),
  FOREIGN KEY (tenant_id,request_id) REFERENCES archava_crm.commands(tenant_id,request_id)
);
CREATE FUNCTION archava_crm.prevent_audit_change() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN RAISE EXCEPTION 'CRM audit is append-only'; END $$;
CREATE TRIGGER crm_audit_immutable BEFORE UPDATE OR DELETE ON archava_crm.audit
  FOR EACH ROW EXECUTE FUNCTION archava_crm.prevent_audit_change();

ALTER TABLE archava_crm.memberships ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_crm.memberships FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_crm.contacts ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_crm.contacts FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_crm.activities ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_crm.activities FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_crm.followups ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_crm.followups FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_crm.commands ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_crm.commands FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_crm.audit ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_crm.audit FORCE ROW LEVEL SECURITY;

CREATE POLICY crm_tenant_memberships ON archava_crm.memberships
 USING (tenant_id = current_setting('archava.tenant_id',true)) WITH CHECK (tenant_id = current_setting('archava.tenant_id',true));
CREATE POLICY crm_tenant_contacts ON archava_crm.contacts
 USING (tenant_id = current_setting('archava.tenant_id',true)) WITH CHECK (tenant_id = current_setting('archava.tenant_id',true));
CREATE POLICY crm_tenant_activities ON archava_crm.activities
 USING (tenant_id = current_setting('archava.tenant_id',true)) WITH CHECK (tenant_id = current_setting('archava.tenant_id',true));
CREATE POLICY crm_tenant_followups ON archava_crm.followups
 USING (tenant_id = current_setting('archava.tenant_id',true)) WITH CHECK (tenant_id = current_setting('archava.tenant_id',true));
CREATE POLICY crm_tenant_commands ON archava_crm.commands
 USING (tenant_id = current_setting('archava.tenant_id',true)) WITH CHECK (tenant_id = current_setting('archava.tenant_id',true));
CREATE POLICY crm_tenant_audit ON archava_crm.audit
 USING (tenant_id = current_setting('archava.tenant_id',true)) WITH CHECK (tenant_id = current_setting('archava.tenant_id',true));

-- Row locking needs UPDATE privileges. This bounded read function locks membership
-- for authorization without granting the app permission to change memberships.
CREATE FUNCTION archava_crm.lock_membership(wanted_tenant text, wanted_principal text)
RETURNS SETOF archava_crm.memberships LANGUAGE sql SECURITY DEFINER
SET search_path = pg_catalog AS $$
 SELECT * FROM archava_crm.memberships
 WHERE tenant_id=wanted_tenant AND principal_id=wanted_principal
   AND wanted_tenant=current_setting('archava.tenant_id',true)
 FOR SHARE
$$;
REVOKE ALL ON FUNCTION archava_crm.lock_membership(text,text) FROM PUBLIC;
