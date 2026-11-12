-- Extend the installed/runtime readiness fence to explicitly configured company books.
CREATE OR REPLACE FUNCTION archava_control.install_runtime_write_guards() RETURNS void
LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE relation record;
BEGIN
 FOR relation IN SELECT n.nspname,c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname IN('archava_commerce','archava_identity','archava_crm','archava_purchasing','archava_foodservice','archava_organizations','archava_proposals','archava_projects','archava_scheduling','archava_accommodation','archava_assets','archava_billing','archava_subscriptions','archava_attendance','archava_support','archava_laundry','archava_field','archava_fulfillment','archava_edu','archava_provider','archava_owner','archava_cockpit','archava_orbz','archava_accounting')
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

CREATE OR REPLACE FUNCTION archava_control.write_guards_complete() RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog AS $$
 SELECT NOT EXISTS(SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname IN('archava_commerce','archava_identity','archava_crm','archava_purchasing','archava_foodservice','archava_organizations','archava_proposals','archava_projects','archava_scheduling','archava_accommodation','archava_assets','archava_billing','archava_subscriptions','archava_attendance','archava_support','archava_laundry','archava_field','archava_fulfillment','archava_edu','archava_provider','archava_owner','archava_cockpit','archava_orbz','archava_accounting') AND c.relkind='r' AND c.relname<>'schema_migrations'
 AND NOT EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid=c.oid AND t.tgname='archava_runtime_write_fence'
  AND t.tgfoid='archava_control.guard_runtime_write()'::regprocedure AND t.tgenabled='O' AND NOT t.tgisinternal AND t.tgtype=62 AND t.tgnargs=0 AND t.tgqual IS NULL AND t.tgattr=''::int2vector))
$$;
REVOKE ALL ON FUNCTION archava_control.write_guards_complete() FROM PUBLIC;
