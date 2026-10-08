CREATE TABLE archava_identity.users (
  id text PRIMARY KEY,
  username text NOT NULL UNIQUE CHECK (username=lower(username) AND length(username)<=254),
  password_hash text NOT NULL,
  active boolean NOT NULL DEFAULT true,
  version integer NOT NULL DEFAULT 1 CHECK (version>0),
  created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE archava_identity.sessions (
  token_hash text PRIMARY KEY CHECK (length(token_hash)=64),
  user_id text NOT NULL REFERENCES archava_identity.users(id),
  user_version integer NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  last_seen_at timestamptz NOT NULL DEFAULT now(),
  expires_at timestamptz NOT NULL DEFAULT now()+interval '8 hours'
);
CREATE INDEX identity_sessions_user ON archava_identity.sessions(user_id);
CREATE INDEX identity_sessions_expiry ON archava_identity.sessions(expires_at);
CREATE TABLE archava_identity.login_buckets (
  key text PRIMARY KEY CHECK (length(key)=64),
  attempts integer NOT NULL CHECK (attempts>0),
  window_start timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE archava_identity.events (
  id uuid PRIMARY KEY,
  user_id text REFERENCES archava_identity.users(id),
  kind text NOT NULL CHECK (kind IN ('login','logout','password_change','provision','password_reset')),
  occurred_at timestamptz NOT NULL DEFAULT now()
);
CREATE FUNCTION archava_identity.prevent_event_change() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN RAISE EXCEPTION 'Identity events are append-only'; END $$;
CREATE TRIGGER identity_events_immutable BEFORE UPDATE OR DELETE ON archava_identity.events
  FOR EACH ROW EXECUTE FUNCTION archava_identity.prevent_event_change();
