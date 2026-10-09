-- Preserve historical audit rows. Service actions record both actor and human initiator.
ALTER TABLE archava_crm.commands ADD COLUMN initiator_id text;
ALTER TABLE archava_crm.commands ADD FOREIGN KEY(tenant_id,initiator_id)
  REFERENCES archava_crm.memberships(tenant_id,principal_id);
ALTER TABLE archava_crm.audit ADD COLUMN initiator_id text;
ALTER TABLE archava_crm.audit ADD FOREIGN KEY(tenant_id,initiator_id)
  REFERENCES archava_crm.memberships(tenant_id,principal_id);
