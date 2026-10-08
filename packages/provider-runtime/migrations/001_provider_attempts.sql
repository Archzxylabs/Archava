CREATE TABLE archava_provider.buckets (
  bucket_id text PRIMARY KEY,
  provider_id text NOT NULL,
  model text NOT NULL,
  daily_limit integer NOT NULL CHECK (daily_limit BETWEEN 1 AND 1000),
  minimum_interval_ms integer NOT NULL CHECK (minimum_interval_ms BETWEEN 0 AND 60000),
  lease_ms integer NOT NULL CHECK (lease_ms BETWEEN 1000 AND 120000),
  usage_day date NOT NULL,
  used integer NOT NULL DEFAULT 0 CHECK (used >= 0 AND used <= daily_limit),
  last_started_at timestamptz,
  active_attempt uuid,
  lease_until timestamptz,
  hold_until timestamptz,
  CHECK ((active_attempt IS NULL) = (lease_until IS NULL))
);
CREATE TABLE archava_provider.attempts (
  bucket_id text NOT NULL REFERENCES archava_provider.buckets(bucket_id),
  attempt_id uuid NOT NULL,
  started_at timestamptz NOT NULL,
  finished_at timestamptz,
  elapsed_ms integer CHECK (elapsed_ms BETWEEN 0 AND 2147483647),
  state text NOT NULL CHECK (state IN ('pending','succeeded','failed','unknown')),
  failure_code text CHECK (failure_code IN ('provider_failed','timeout','provider_unavailable','invalid_response','rate_limited','plan_mismatch','synthetic_only','budget_exhausted','busy','interrupted')),
  http_status integer CHECK (http_status BETWEEN 100 AND 599),
  PRIMARY KEY(bucket_id,attempt_id),
  CHECK ((state='pending') = (finished_at IS NULL)),
  CHECK ((state='pending') = (elapsed_ms IS NULL)),
  CHECK ((state IN ('failed','unknown')) = (failure_code IS NOT NULL))
);
CREATE INDEX provider_attempts_recent ON archava_provider.attempts(bucket_id,started_at DESC,attempt_id);
ALTER TABLE archava_provider.buckets ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_provider.buckets FORCE ROW LEVEL SECURITY;
ALTER TABLE archava_provider.attempts ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_provider.attempts FORCE ROW LEVEL SECURITY;
CREATE POLICY provider_bucket_scope ON archava_provider.buckets
  USING (bucket_id=current_setting('archava.provider_bucket',true))
  WITH CHECK (bucket_id=current_setting('archava.provider_bucket',true));
CREATE POLICY provider_attempt_scope ON archava_provider.attempts
  USING (bucket_id=current_setting('archava.provider_bucket',true))
  WITH CHECK (bucket_id=current_setting('archava.provider_bucket',true));
CREATE FUNCTION archava_provider.preserve_attempt() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF OLD.state <> 'pending' OR NEW.bucket_id <> OLD.bucket_id OR NEW.attempt_id <> OLD.attempt_id
    OR NEW.started_at <> OLD.started_at OR NEW.state='pending' THEN
    RAISE EXCEPTION 'Provider attempt transition refused';
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER provider_attempt_final BEFORE UPDATE ON archava_provider.attempts
  FOR EACH ROW EXECUTE FUNCTION archava_provider.preserve_attempt();
