-- Add permission names without granting them to existing memberships.
ALTER TABLE archava_crm.memberships DROP CONSTRAINT memberships_permissions_check;
ALTER TABLE archava_crm.memberships ADD CONSTRAINT crm_membership_permissions
 CHECK (permissions <@ ARRAY['contacts.read','contacts.write','activities.read','activities.write','followups.read','followups.write','audit.read','pipeline.read','pipeline.write']::text[]);
ALTER TABLE archava_crm.contacts ADD CONSTRAINT crm_contact_owner_reference UNIQUE(tenant_id,id,owner_id);
CREATE TABLE archava_crm.opportunities (
 tenant_id text NOT NULL, id uuid NOT NULL, contact_id uuid NOT NULL, owner_id text NOT NULL,
 title text NOT NULL CHECK (length(title) BETWEEN 1 AND 160),
 stage text NOT NULL DEFAULT 'new' CHECK (stage IN ('new','qualified','proposal','won','lost')),
 revision integer NOT NULL DEFAULT 1 CHECK (revision > 0),
 loss_reason text CHECK (length(loss_reason) BETWEEN 1 AND 1000),
 created_at timestamptz NOT NULL DEFAULT now(), updated_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY (tenant_id,id),
 FOREIGN KEY (tenant_id,contact_id,owner_id) REFERENCES archava_crm.contacts(tenant_id,id,owner_id),
 CHECK ((stage='lost' AND loss_reason IS NOT NULL) OR (stage<>'lost' AND loss_reason IS NULL))
);
CREATE TABLE archava_crm.opportunity_transitions (
 tenant_id text NOT NULL, request_id uuid NOT NULL, opportunity_id uuid NOT NULL,
 from_stage text CHECK (from_stage IN ('new','qualified','proposal','won','lost')),
 to_stage text NOT NULL CHECK (to_stage IN ('new','qualified','proposal','won','lost')),
 reason text CHECK (length(reason) BETWEEN 1 AND 1000), actor_id text NOT NULL,
 revision integer NOT NULL CHECK (revision > 0), occurred_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY (tenant_id,request_id), UNIQUE(tenant_id,opportunity_id,revision),
 FOREIGN KEY (tenant_id,request_id) REFERENCES archava_crm.commands(tenant_id,request_id),
 FOREIGN KEY (tenant_id,opportunity_id) REFERENCES archava_crm.opportunities(tenant_id,id),
 FOREIGN KEY (tenant_id,actor_id) REFERENCES archava_crm.memberships(tenant_id,principal_id),
 CHECK ((revision=1 AND from_stage IS NULL AND to_stage='new') OR
   (revision>1 AND from_stage IS NOT NULL AND from_stage<>to_stage)),
 CHECK (to_stage<>'lost' OR reason IS NOT NULL)
);
CREATE INDEX crm_opportunities_stage ON archava_crm.opportunities(tenant_id,stage,id);
CREATE INDEX crm_opportunities_owner ON archava_crm.opportunities(tenant_id,owner_id,id);
CREATE INDEX crm_opportunities_contact ON archava_crm.opportunities(tenant_id,contact_id,id);
CREATE TRIGGER crm_opportunity_history_immutable BEFORE UPDATE OR DELETE ON archava_crm.opportunity_transitions
 FOR EACH ROW EXECUTE FUNCTION archava_crm.prevent_audit_change();
ALTER TABLE archava_crm.opportunities ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_crm.opportunities FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_crm.opportunity_transitions ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_crm.opportunity_transitions FORCE ROW LEVEL SECURITY;
CREATE POLICY crm_tenant_opportunities ON archava_crm.opportunities
 USING (tenant_id=current_setting('archava.tenant_id',true)) WITH CHECK (tenant_id=current_setting('archava.tenant_id',true));
CREATE POLICY crm_tenant_opportunity_history ON archava_crm.opportunity_transitions
 USING (tenant_id=current_setting('archava.tenant_id',true)) WITH CHECK (tenant_id=current_setting('archava.tenant_id',true));
