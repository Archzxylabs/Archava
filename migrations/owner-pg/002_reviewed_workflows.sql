-- Server-bound reviewed input and durable receipts for imports and feed actions.
CREATE TABLE archava_owner.reviewed_workflows (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  principal_id text NOT NULL,
  kind text NOT NULL CHECK (kind IN ('import','feed')),
  plan_hash text NOT NULL CHECK (plan_hash ~ '^[0-9a-f]{64}$'),
  plan jsonb NOT NULL CHECK (jsonb_typeof(plan)='object'),
  receipt jsonb CHECK (receipt IS NULL OR jsonb_typeof(receipt)='object'),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  expires_at timestamptz NOT NULL DEFAULT (clock_timestamp()+interval '30 minutes'),
  PRIMARY KEY (tenant_id,id),
  CHECK (expires_at>created_at)
);
ALTER TABLE archava_owner.reviewed_workflows ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_owner.reviewed_workflows FORCE ROW LEVEL SECURITY;
CREATE POLICY reviewed_workflow_tenant ON archava_owner.reviewed_workflows
 USING (tenant_id=current_setting('archava.tenant_id',true))
 WITH CHECK (tenant_id=current_setting('archava.tenant_id',true));
CREATE FUNCTION archava_owner.guard_reviewed_workflow() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Reviewed workflow history is immutable'; END IF;
 IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.id IS DISTINCT FROM OLD.id
 OR NEW.principal_id IS DISTINCT FROM OLD.principal_id OR NEW.kind IS DISTINCT FROM OLD.kind
 OR NEW.plan_hash IS DISTINCT FROM OLD.plan_hash OR NEW.plan IS DISTINCT FROM OLD.plan
 OR NEW.created_at IS DISTINCT FROM OLD.created_at OR NEW.expires_at IS DISTINCT FROM OLD.expires_at
 OR OLD.receipt IS NOT NULL OR NEW.receipt IS NULL
 THEN RAISE EXCEPTION 'Only a first receipt may be appended to a reviewed workflow'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER reviewed_workflow_guard BEFORE UPDATE OR DELETE ON archava_owner.reviewed_workflows
 FOR EACH ROW EXECUTE FUNCTION archava_owner.guard_reviewed_workflow();
