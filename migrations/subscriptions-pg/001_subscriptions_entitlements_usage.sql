CREATE TABLE archava_subscriptions.memberships (
  tenant_id text NOT NULL,
  principal_id text NOT NULL,
  display_name text NOT NULL DEFAULT '' CHECK(length(display_name) <= 254),
  kind text NOT NULL CHECK(kind IN ('human', 'service')),
  permissions text[] NOT NULL CHECK(permissions <@ ARRAY['subscriptions.read', 'subscriptions.write', 'subscriptions.payment', 'subscriptions.usage', 'audit.read']::text[]),
  scope text NOT NULL CHECK(scope IN ('all', 'assigned')),
  active boolean NOT NULL DEFAULT true,
  PRIMARY KEY(tenant_id, principal_id)
);

CREATE TABLE archava_subscriptions.subscriptions (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  contact_id text NOT NULL CHECK(length(contact_id) <= 80),
  plan_code text NOT NULL CHECK(length(plan_code) BETWEEN 1 AND 100),
  plan_name text NOT NULL CHECK(length(plan_name) BETWEEN 1 AND 200),
  status text NOT NULL DEFAULT 'draft' CHECK(status IN ('draft', 'active', 'past_due', 'paused', 'canceled', 'expired')),
  billing_interval text NOT NULL CHECK(billing_interval IN ('weekly', 'monthly', 'quarterly', 'yearly')),
  unit_price integer NOT NULL CHECK(unit_price >= 0),
  currency text NOT NULL CHECK(length(currency) = 3),
  quantity integer NOT NULL DEFAULT 1 CHECK(quantity > 0),
  current_period_start timestamptz,
  current_period_end timestamptz,
  cancel_at_period_end boolean NOT NULL DEFAULT false,
  payment_reference text CHECK(payment_reference IS NULL OR length(payment_reference) <= 200),
  owner_id text NOT NULL,
  revision integer NOT NULL DEFAULT 1 CHECK(revision > 0),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, id),
  FOREIGN KEY(tenant_id, owner_id) REFERENCES archava_subscriptions.memberships(tenant_id, principal_id)
);

CREATE INDEX subscription_list ON archava_subscriptions.subscriptions(tenant_id, status, id);
CREATE INDEX subscription_contact ON archava_subscriptions.subscriptions(tenant_id, contact_id, id);
CREATE INDEX subscription_owner ON archava_subscriptions.subscriptions(tenant_id, owner_id, id);

CREATE TABLE archava_subscriptions.entitlements (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  subscription_id uuid,
  contact_id text NOT NULL CHECK(length(contact_id) <= 80),
  feature_code text NOT NULL CHECK(length(feature_code) BETWEEN 1 AND 100),
  feature_name text NOT NULL CHECK(length(feature_name) BETWEEN 1 AND 200),
  entitlement_type text NOT NULL CHECK(entitlement_type IN ('metered', 'seat_count', 'boolean_access', 'pass_bundle')),
  allocated_quantity integer CHECK(allocated_quantity IS NULL OR allocated_quantity >= 0),
  consumed_quantity integer NOT NULL DEFAULT 0 CHECK(consumed_quantity >= 0),
  status text NOT NULL DEFAULT 'active' CHECK(status IN ('active', 'suspended', 'revoked', 'exhausted')),
  valid_from timestamptz NOT NULL DEFAULT clock_timestamp(),
  valid_until timestamptz,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, id),
  FOREIGN KEY(tenant_id, subscription_id) REFERENCES archava_subscriptions.subscriptions(tenant_id, id) ON DELETE CASCADE,
  CONSTRAINT entitlement_quota_limit CHECK (allocated_quantity IS NULL OR consumed_quantity <= allocated_quantity)
);

CREATE INDEX entitlement_sub ON archava_subscriptions.entitlements(tenant_id, subscription_id, id);
CREATE INDEX entitlement_contact ON archava_subscriptions.entitlements(tenant_id, contact_id, id);
CREATE INDEX entitlement_feature ON archava_subscriptions.entitlements(tenant_id, feature_code, id);

CREATE TABLE archava_subscriptions.usage_events (
  tenant_id text NOT NULL,
  id uuid NOT NULL,
  entitlement_id uuid NOT NULL,
  subscription_id uuid,
  contact_id text NOT NULL CHECK(length(contact_id) <= 80),
  units_consumed integer NOT NULL CHECK(units_consumed > 0),
  idempotency_key text NOT NULL CHECK(length(idempotency_key) BETWEEN 1 AND 100),
  evidence_ref text CHECK(evidence_ref IS NULL OR length(evidence_ref) <= 300),
  recorded_by text NOT NULL,
  recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, id),
  FOREIGN KEY(tenant_id, entitlement_id) REFERENCES archava_subscriptions.entitlements(tenant_id, id),
  CONSTRAINT usage_idempotency UNIQUE (tenant_id, entitlement_id, idempotency_key)
);

CREATE INDEX usage_history ON archava_subscriptions.usage_events(tenant_id, entitlement_id, recorded_at DESC);

CREATE TABLE archava_subscriptions.commands (
  tenant_id text NOT NULL,
  request_id uuid NOT NULL,
  actor_id text NOT NULL,
  fingerprint text NOT NULL,
  receipt jsonb,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, request_id),
  FOREIGN KEY(tenant_id, actor_id) REFERENCES archava_subscriptions.memberships(tenant_id, principal_id)
);

CREATE TABLE archava_subscriptions.audit (
  tenant_id text NOT NULL,
  request_id uuid NOT NULL,
  actor_id text NOT NULL,
  operation text NOT NULL,
  subscription_id uuid,
  revision integer NOT NULL CHECK(revision > 0),
  reason text CHECK(reason IS NULL OR length(reason) <= 2000),
  occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(tenant_id, request_id),
  FOREIGN KEY(tenant_id, request_id) REFERENCES archava_subscriptions.commands(tenant_id, request_id)
);

CREATE INDEX subscription_audit_history ON archava_subscriptions.audit(tenant_id, subscription_id, revision DESC);

CREATE FUNCTION archava_subscriptions.immutable_history() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  RAISE EXCEPTION 'Subscription audit history is immutable';
END
$$;

CREATE TRIGGER audit_immutable BEFORE UPDATE OR DELETE ON archava_subscriptions.audit FOR EACH ROW EXECUTE FUNCTION archava_subscriptions.immutable_history();

CREATE FUNCTION archava_subscriptions.guard_command() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF TG_OP='DELETE' OR OLD.receipt IS NOT NULL OR NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.request_id IS DISTINCT FROM OLD.request_id OR NEW.actor_id IS DISTINCT FROM OLD.actor_id OR NEW.fingerprint IS DISTINCT FROM OLD.fingerprint OR NEW.created_at IS DISTINCT FROM OLD.created_at THEN
    RAISE EXCEPTION 'Subscription command is immutable';
  END IF;
  RETURN NEW;
END
$$;

CREATE TRIGGER command_guard BEFORE UPDATE OR DELETE ON archava_subscriptions.commands FOR EACH ROW EXECUTE FUNCTION archava_subscriptions.guard_command();

DO $$ DECLARE owned text; BEGIN
  FOREACH owned IN ARRAY ARRAY['memberships', 'subscriptions', 'entitlements', 'usage_events', 'commands', 'audit'] LOOP
    EXECUTE format('ALTER TABLE archava_subscriptions.%I ENABLE ROW LEVEL SECURITY', owned);
    EXECUTE format('ALTER TABLE archava_subscriptions.%I FORCE ROW LEVEL SECURITY', owned);
    EXECUTE format('CREATE POLICY subscription_tenant ON archava_subscriptions.%I USING(tenant_id=current_setting(''archava.tenant_id'',true)) WITH CHECK(tenant_id=current_setting(''archava.tenant_id'',true))', owned);
  END LOOP;
END $$;

CREATE FUNCTION archava_subscriptions.lock_membership(wanted_tenant text, wanted_principal text)
RETURNS SETOF archava_subscriptions.memberships LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog AS $$
  SELECT * FROM archava_subscriptions.memberships WHERE tenant_id=wanted_tenant AND principal_id=wanted_principal AND wanted_tenant=current_setting('archava.tenant_id',true) FOR SHARE
$$;

REVOKE ALL ON FUNCTION archava_subscriptions.lock_membership(text, text) FROM PUBLIC;
