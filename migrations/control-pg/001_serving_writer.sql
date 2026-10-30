-- Process-fixed serving epoch enforced at every runtime persistence boundary.
CREATE TABLE IF NOT EXISTS archava_control.writer_lease(
 lease_name text PRIMARY KEY,epoch bigint NOT NULL DEFAULT 0,owner_token text NOT NULL,
 drained boolean NOT NULL DEFAULT false,compat_window_seconds integer NOT NULL DEFAULT 300,
 taken_at timestamptz NOT NULL DEFAULT now(),updated_at timestamptz NOT NULL DEFAULT now()
);
ALTER TABLE archava_control.writer_lease ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_control.writer_lease FORCE ROW LEVEL SECURITY;
-- Global operational metadata; access is restricted by role grants, not tenant IDs.
CREATE POLICY serving_control ON archava_control.writer_lease
 USING(true) WITH CHECK(true);
REVOKE ALL ON archava_control.writer_lease FROM PUBLIC;

CREATE FUNCTION archava_control.take_serving_writer(next_token text,takeover boolean)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog SET lock_timeout='3s' AS $$
DECLARE prior archava_control.writer_lease;next_epoch bigint;
BEGIN
 IF next_token IS NULL OR next_token !~ '^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$' THEN
  RAISE EXCEPTION 'Invalid writer token' USING ERRCODE='22023';
 END IF;
 PERFORM pg_advisory_xact_lock(hashtext('archava-serving-writer-takeover'));
 SELECT * INTO prior FROM archava_control.writer_lease WHERE lease_name='archava-serving' FOR UPDATE;
 IF FOUND THEN
  IF NOT prior.drained AND NOT coalesce(takeover,false) THEN
   RAISE EXCEPTION 'Serving writer active; drain or explicitly take over' USING ERRCODE='55006';
  END IF;
  UPDATE archava_control.writer_lease SET epoch=epoch+1,owner_token=next_token,drained=false,taken_at=clock_timestamp(),updated_at=clock_timestamp()
   WHERE lease_name='archava-serving' RETURNING epoch INTO next_epoch;
 ELSE
  INSERT INTO archava_control.writer_lease(lease_name,epoch,owner_token) VALUES('archava-serving',1,next_token) RETURNING epoch INTO next_epoch;
 END IF;
 RETURN next_epoch;
END $$;
REVOKE ALL ON FUNCTION archava_control.take_serving_writer(text,boolean) FROM PUBLIC;

CREATE FUNCTION archava_control.require_serving_writer() RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog SET lock_timeout='3s' AS $$
DECLARE held archava_control.writer_lease;
BEGIN
 SELECT * INTO held FROM archava_control.writer_lease WHERE lease_name='archava-serving' FOR SHARE;
 IF NOT FOUND OR held.drained
  OR current_setting('archava.writer_epoch',true) IS DISTINCT FROM held.epoch::text
  OR current_setting('archava.writer_token',true) IS DISTINCT FROM held.owner_token THEN
  RAISE EXCEPTION 'Serving writer missing, stale or drained' USING ERRCODE='57P03';
 END IF;
END $$;
REVOKE ALL ON FUNCTION archava_control.require_serving_writer() FROM PUBLIC;

CREATE FUNCTION archava_control.drain_serving_writer(held_epoch bigint,held_token text) RETURNS boolean
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog SET lock_timeout='3s' AS $$
BEGIN
 UPDATE archava_control.writer_lease SET drained=true,updated_at=clock_timestamp()
  WHERE lease_name='archava-serving' AND epoch=held_epoch AND owner_token=held_token;
 RETURN FOUND;
END $$;
REVOKE ALL ON FUNCTION archava_control.drain_serving_writer(bigint,text) FROM PUBLIC;

CREATE FUNCTION archava_control.guard_runtime_write() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog AS $$
DECLARE table_owner oid;privileged boolean;
BEGIN
 SELECT relowner INTO table_owner FROM pg_class WHERE oid=TG_RELID;
 SELECT rolsuper INTO privileged FROM pg_roles WHERE rolname=session_user;
 -- Migrations/bootstrap/restores use a dedicated owner/admin session. Runtime
 -- does not become exempt when a ledger-owned trigger calls another trigger.
 IF coalesce(privileged,false) OR pg_has_role(session_user,table_owner,'USAGE') THEN RETURN NULL;END IF;
 PERFORM archava_control.require_serving_writer();
 RETURN NULL;
END $$;
REVOKE ALL ON FUNCTION archava_control.guard_runtime_write() FROM PUBLIC;

CREATE FUNCTION archava_control.install_runtime_write_guards() RETURNS void
LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE relation record;
BEGIN
 FOR relation IN SELECT n.nspname,c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname IN('archava_commerce','archava_identity','archava_crm','archava_purchasing','archava_foodservice','archava_organizations','archava_proposals','archava_projects','archava_scheduling','archava_accommodation','archava_assets','archava_billing','archava_subscriptions','archava_attendance','archava_support','archava_laundry','archava_field','archava_fulfillment','archava_edu','archava_provider','archava_owner','archava_cockpit','archava_orbz')
   AND c.relkind='r' AND c.relname<>'schema_migrations' ORDER BY n.nspname,c.relname
 LOOP
  IF EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid=format('%I.%I',relation.nspname,relation.relname)::regclass AND t.tgname='archava_runtime_write_fence' AND (t.tgfoid<>'archava_control.guard_runtime_write()'::regprocedure OR t.tgenabled<>'O' OR t.tgisinternal OR t.tgtype<>62 OR t.tgnargs<>0 OR t.tgqual IS NOT NULL OR t.tgattr<>''::int2vector)) THEN
   RAISE EXCEPTION 'Runtime write guard was replaced or disabled' USING ERRCODE='55000';
  END IF;
  IF NOT EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid=format('%I.%I',relation.nspname,relation.relname)::regclass AND t.tgname='archava_runtime_write_fence') THEN
   EXECUTE format('CREATE TRIGGER archava_runtime_write_fence BEFORE INSERT OR UPDATE OR DELETE OR TRUNCATE ON %I.%I FOR EACH STATEMENT EXECUTE FUNCTION archava_control.guard_runtime_write()',relation.nspname,relation.relname);
  END IF;
 END LOOP;
END $$;
REVOKE ALL ON FUNCTION archava_control.install_runtime_write_guards() FROM PUBLIC;
SELECT archava_control.install_runtime_write_guards();

CREATE FUNCTION archava_control.write_guards_complete() RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog AS $$
 SELECT NOT EXISTS(SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname IN('archava_commerce','archava_identity','archava_crm','archava_purchasing','archava_foodservice','archava_organizations','archava_proposals','archava_projects','archava_scheduling','archava_accommodation','archava_assets','archava_billing','archava_subscriptions','archava_attendance','archava_support','archava_laundry','archava_field','archava_fulfillment','archava_edu','archava_provider','archava_owner','archava_cockpit','archava_orbz') AND c.relkind='r' AND c.relname<>'schema_migrations'
 AND NOT EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid=c.oid AND t.tgname='archava_runtime_write_fence'
  AND t.tgfoid='archava_control.guard_runtime_write()'::regprocedure AND t.tgenabled='O' AND NOT t.tgisinternal AND t.tgtype=62 AND t.tgnargs=0 AND t.tgqual IS NULL AND t.tgattr=''::int2vector))
$$;
REVOKE ALL ON FUNCTION archava_control.write_guards_complete() FROM PUBLIC;
