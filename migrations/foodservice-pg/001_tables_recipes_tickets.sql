CREATE TABLE archava_foodservice.memberships (
 tenant_id text NOT NULL,principal_id text NOT NULL,kind text NOT NULL CHECK(kind IN ('human','service')),
 permissions text[] NOT NULL CHECK(permissions <@ ARRAY['tables.read','tables.write','menu.read','menu.write','tickets.read','tickets.write','kitchen.write','audit.read']::text[]),scope text NOT NULL CHECK(scope IN ('all','assigned')),active boolean NOT NULL DEFAULT true,PRIMARY KEY(tenant_id,principal_id)
);
CREATE TABLE archava_foodservice.tables (
 tenant_id text NOT NULL,id uuid NOT NULL,code text NOT NULL CHECK(code ~ '^[A-Z0-9][A-Z0-9_-]{0,19}$'),title text NOT NULL CHECK(length(title) BETWEEN 1 AND 160),seats integer NOT NULL CHECK(seats BETWEEN 1 AND 40),active boolean NOT NULL DEFAULT true,
 revision integer NOT NULL DEFAULT 1 CHECK(revision>0),created_at timestamptz NOT NULL DEFAULT clock_timestamp(),updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,code)
);
CREATE TABLE archava_foodservice.menu_items (
 tenant_id text NOT NULL,id uuid NOT NULL,station text NOT NULL CHECK(station IN ('kitchen','bar')),recipe_version integer NOT NULL DEFAULT 1 CHECK(recipe_version>0),revision integer NOT NULL DEFAULT 1 CHECK(revision>0),created_at timestamptz NOT NULL DEFAULT clock_timestamp(),updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,id),FOREIGN KEY(tenant_id,id) REFERENCES archava_commerce.products(tenant_id,id)
);
CREATE TABLE archava_foodservice.recipe_versions (
 tenant_id text NOT NULL,menu_id uuid NOT NULL,version integer NOT NULL CHECK(version>0),sealed boolean NOT NULL DEFAULT false,created_at timestamptz NOT NULL DEFAULT clock_timestamp(),PRIMARY KEY(tenant_id,menu_id,version),FOREIGN KEY(tenant_id,menu_id) REFERENCES archava_foodservice.menu_items(tenant_id,id)
);
ALTER TABLE archava_foodservice.menu_items ADD CONSTRAINT current_recipe FOREIGN KEY(tenant_id,id,recipe_version) REFERENCES archava_foodservice.recipe_versions(tenant_id,menu_id,version) DEFERRABLE INITIALLY DEFERRED;
CREATE TABLE archava_foodservice.recipe_components (
 tenant_id text NOT NULL,menu_id uuid NOT NULL,version integer NOT NULL,product_id uuid NOT NULL,quantity integer NOT NULL CHECK(quantity BETWEEN 1 AND 1000000),PRIMARY KEY(tenant_id,menu_id,version,product_id),FOREIGN KEY(tenant_id,menu_id,version) REFERENCES archava_foodservice.recipe_versions(tenant_id,menu_id,version),FOREIGN KEY(tenant_id,product_id) REFERENCES archava_commerce.products(tenant_id,id)
);
CREATE TABLE archava_foodservice.tickets (
 tenant_id text NOT NULL,id uuid NOT NULL,owner_id text NOT NULL,order_id uuid NOT NULL,mode text NOT NULL CHECK(mode IN ('dine_in','takeaway')),table_id uuid,guests integer NOT NULL CHECK(guests BETWEEN 1 AND 40),state text NOT NULL DEFAULT 'draft' CHECK(state IN ('draft','queued','preparing','ready','served','closed','cancelled')),prepared_at timestamptz,cancellation_reason text CHECK(length(cancellation_reason) BETWEEN 1 AND 2000),
 revision integer NOT NULL DEFAULT 1 CHECK(revision>0),created_at timestamptz NOT NULL DEFAULT clock_timestamp(),updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,order_id),UNIQUE(tenant_id,id,order_id),FOREIGN KEY(tenant_id,owner_id) REFERENCES archava_foodservice.memberships(tenant_id,principal_id),FOREIGN KEY(tenant_id,order_id) REFERENCES archava_commerce.orders(tenant_id,id),FOREIGN KEY(tenant_id,table_id) REFERENCES archava_foodservice.tables(tenant_id,id),CHECK((mode='dine_in')=(table_id IS NOT NULL)),CHECK(state NOT IN ('preparing','ready','served','closed') OR prepared_at IS NOT NULL),CHECK((state='cancelled')=(cancellation_reason IS NOT NULL))
);
CREATE UNIQUE INDEX one_open_ticket_per_table ON archava_foodservice.tickets(tenant_id,table_id) WHERE table_id IS NOT NULL AND state NOT IN ('closed','cancelled');
CREATE INDEX ticket_worklist ON archava_foodservice.tickets(tenant_id,owner_id,state,id);
CREATE TABLE archava_foodservice.ticket_lines (
 tenant_id text NOT NULL,ticket_id uuid NOT NULL,order_id uuid NOT NULL,order_line_id uuid NOT NULL,menu_id uuid NOT NULL,recipe_version integer NOT NULL,quantity integer NOT NULL CHECK(quantity BETWEEN 1 AND 100),station text NOT NULL CHECK(station IN ('kitchen','bar')),PRIMARY KEY(tenant_id,ticket_id,menu_id),FOREIGN KEY(tenant_id,ticket_id,order_id) REFERENCES archava_foodservice.tickets(tenant_id,id,order_id),FOREIGN KEY(tenant_id,order_id,order_line_id) REFERENCES archava_commerce.order_lines(tenant_id,order_id,id),FOREIGN KEY(tenant_id,menu_id,recipe_version) REFERENCES archava_foodservice.recipe_versions(tenant_id,menu_id,version)
);
CREATE TABLE archava_foodservice.commands (
 tenant_id text NOT NULL,request_id uuid NOT NULL,actor_id text NOT NULL,fingerprint text NOT NULL,receipt jsonb,created_at timestamptz NOT NULL DEFAULT clock_timestamp(),PRIMARY KEY(tenant_id,request_id),FOREIGN KEY(tenant_id,actor_id) REFERENCES archava_foodservice.memberships(tenant_id,principal_id)
);
CREATE TABLE archava_foodservice.audit (
 tenant_id text NOT NULL,request_id uuid NOT NULL,actor_id text NOT NULL,operation text NOT NULL,record_id uuid NOT NULL,revision integer NOT NULL CHECK(revision>0),reason text CHECK(length(reason) BETWEEN 1 AND 2000),occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),PRIMARY KEY(tenant_id,request_id),FOREIGN KEY(tenant_id,request_id) REFERENCES archava_foodservice.commands(tenant_id,request_id)
);
CREATE INDEX food_history ON archava_foodservice.audit(tenant_id,record_id,occurred_at DESC,request_id);
CREATE FUNCTION archava_foodservice.immutable_record() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'Food-service history is immutable';END $$;
CREATE FUNCTION archava_foodservice.seal_recipe() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF TG_OP='DELETE' OR OLD.sealed OR NOT NEW.sealed OR NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.menu_id IS DISTINCT FROM OLD.menu_id OR NEW.version IS DISTINCT FROM OLD.version OR NEW.created_at IS DISTINCT FROM OLD.created_at THEN RAISE EXCEPTION 'Recipe version is immutable';END IF;RETURN NEW;
END $$;
CREATE TRIGGER recipes_immutable BEFORE UPDATE OR DELETE ON archava_foodservice.recipe_versions FOR EACH ROW EXECUTE FUNCTION archava_foodservice.seal_recipe();
CREATE FUNCTION archava_foodservice.guard_component() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 PERFORM version FROM archava_foodservice.recipe_versions WHERE tenant_id=NEW.tenant_id AND menu_id=NEW.menu_id AND version=NEW.version AND NOT sealed FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Sealed recipe cannot gain components' USING ERRCODE='23514';END IF;RETURN NEW;
END $$;
CREATE TRIGGER component_guard BEFORE INSERT ON archava_foodservice.recipe_components FOR EACH ROW EXECUTE FUNCTION archava_foodservice.guard_component();
CREATE TRIGGER components_immutable BEFORE UPDATE OR DELETE ON archava_foodservice.recipe_components FOR EACH ROW EXECUTE FUNCTION archava_foodservice.immutable_record();
CREATE TRIGGER lines_immutable BEFORE UPDATE OR DELETE ON archava_foodservice.ticket_lines FOR EACH ROW EXECUTE FUNCTION archava_foodservice.immutable_record();
CREATE TRIGGER audit_immutable BEFORE UPDATE OR DELETE ON archava_foodservice.audit FOR EACH ROW EXECUTE FUNCTION archava_foodservice.immutable_record();
CREATE FUNCTION archava_foodservice.guard_table() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.id IS DISTINCT FROM OLD.id OR NEW.code IS DISTINCT FROM OLD.code OR NEW.title IS DISTINCT FROM OLD.title OR NEW.seats IS DISTINCT FROM OLD.seats OR NEW.created_at IS DISTINCT FROM OLD.created_at OR NEW.revision<>OLD.revision+1
 THEN RAISE EXCEPTION 'Table identity/revision is immutable' USING ERRCODE='23514';END IF;
 IF NOT NEW.active AND EXISTS(SELECT 1 FROM archava_foodservice.tickets WHERE tenant_id=NEW.tenant_id AND table_id=NEW.id AND state NOT IN ('closed','cancelled')) THEN RAISE EXCEPTION 'Table still occupied' USING ERRCODE='23514';END IF;RETURN NEW;
END $$;
CREATE TRIGGER table_guard BEFORE UPDATE ON archava_foodservice.tables FOR EACH ROW EXECUTE FUNCTION archava_foodservice.guard_table();
CREATE FUNCTION archava_foodservice.guard_menu() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF TG_OP='UPDATE' AND (NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.id IS DISTINCT FROM OLD.id OR NEW.station IS DISTINCT FROM OLD.station OR NEW.created_at IS DISTINCT FROM OLD.created_at OR NEW.revision<>OLD.revision+1 OR NEW.recipe_version NOT IN (OLD.recipe_version,OLD.recipe_version+1)) THEN RAISE EXCEPTION 'Menu identity/revision is immutable' USING ERRCODE='23514';END IF;
 IF NOT EXISTS(SELECT 1 FROM archava_commerce.products WHERE tenant_id=NEW.tenant_id AND id=NEW.id AND managed_by='foodservice' AND NOT track_stock AND unit='unit') THEN RAISE EXCEPTION 'Menu must own a managed portion product' USING ERRCODE='23514';END IF;RETURN NEW;
END $$;
CREATE TRIGGER menu_guard BEFORE INSERT OR UPDATE ON archava_foodservice.menu_items FOR EACH ROW EXECUTE FUNCTION archava_foodservice.guard_menu();
CREATE FUNCTION archava_foodservice.guard_ticket() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF TG_OP='INSERT' THEN
  IF NEW.state<>'draft' OR NEW.prepared_at IS NOT NULL OR NEW.revision<>1 THEN RAISE EXCEPTION 'New ticket must be draft' USING ERRCODE='23514';END IF;
  IF NEW.table_id IS NOT NULL THEN
   PERFORM id FROM archava_foodservice.tables WHERE tenant_id=NEW.tenant_id AND id=NEW.table_id AND active AND seats>=NEW.guests FOR UPDATE;
   IF NOT FOUND THEN RAISE EXCEPTION 'Dining table unavailable' USING ERRCODE='23514';END IF;
  END IF;
 ELSE
  IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.id IS DISTINCT FROM OLD.id OR NEW.owner_id IS DISTINCT FROM OLD.owner_id OR NEW.order_id IS DISTINCT FROM OLD.order_id OR NEW.mode IS DISTINCT FROM OLD.mode OR NEW.table_id IS DISTINCT FROM OLD.table_id OR NEW.guests IS DISTINCT FROM OLD.guests OR NEW.created_at IS DISTINCT FROM OLD.created_at OR NEW.revision<>OLD.revision+1 OR OLD.state IN ('closed','cancelled') OR NOT ((OLD.state='draft' AND NEW.state IN ('queued','cancelled')) OR (OLD.state='queued' AND NEW.state IN ('preparing','cancelled')) OR (OLD.state='preparing' AND NEW.state IN ('ready','cancelled')) OR (OLD.state='ready' AND NEW.state IN ('served','cancelled')) OR (OLD.state='served' AND NEW.state IN ('closed','cancelled')))
  THEN RAISE EXCEPTION 'Ticket transition refused' USING ERRCODE='23514';END IF;
  IF (NEW.state='preparing' AND (OLD.prepared_at IS NOT NULL OR NEW.prepared_at IS NULL)) OR (NEW.state<>'preparing' AND NEW.prepared_at IS DISTINCT FROM OLD.prepared_at) THEN RAISE EXCEPTION 'Preparation evidence is immutable' USING ERRCODE='23514';END IF;
 END IF;RETURN NEW;
END $$;
CREATE TRIGGER ticket_guard BEFORE INSERT OR UPDATE ON archava_foodservice.tickets FOR EACH ROW EXECUTE FUNCTION archava_foodservice.guard_ticket();
CREATE FUNCTION archava_foodservice.guard_line() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF NOT EXISTS(SELECT 1 FROM archava_foodservice.tickets t JOIN archava_commerce.order_lines l ON l.tenant_id=t.tenant_id AND l.order_id=t.order_id WHERE t.tenant_id=NEW.tenant_id AND t.id=NEW.ticket_id AND t.state='draft' AND l.id=NEW.order_line_id AND l.product_id=NEW.menu_id AND l.quantity=NEW.quantity)
 THEN RAISE EXCEPTION 'Ticket line quote binding refused' USING ERRCODE='23514';END IF;RETURN NEW;
END $$;
CREATE TRIGGER line_guard BEFORE INSERT ON archava_foodservice.ticket_lines FOR EACH ROW EXECUTE FUNCTION archava_foodservice.guard_line();
CREATE FUNCTION archava_foodservice.assert_recipe() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE entries integer;
BEGIN
 SELECT count(*) INTO entries FROM archava_foodservice.recipe_components WHERE tenant_id=NEW.tenant_id AND menu_id=NEW.menu_id AND version=NEW.version;
 IF NOT EXISTS(SELECT 1 FROM archava_foodservice.recipe_versions WHERE tenant_id=NEW.tenant_id AND menu_id=NEW.menu_id AND version=NEW.version AND sealed) OR entries NOT BETWEEN 1 AND 20 OR EXISTS(SELECT 1 FROM archava_foodservice.recipe_components c JOIN archava_commerce.products p ON p.tenant_id=c.tenant_id AND p.id=c.product_id WHERE c.tenant_id=NEW.tenant_id AND c.menu_id=NEW.menu_id AND c.version=NEW.version AND (NOT p.track_stock OR p.managed_by IS NOT NULL)) THEN RAISE EXCEPTION 'Recipe requires tracked ingredients' USING ERRCODE='23514';END IF;RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER recipe_complete AFTER INSERT ON archava_foodservice.recipe_versions DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_foodservice.assert_recipe();
CREATE FUNCTION archava_foodservice.assert_ticket() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE t archava_foodservice.tickets;o archava_commerce.orders;entries integer;
BEGIN
 SELECT * INTO t FROM archava_foodservice.tickets WHERE tenant_id=NEW.tenant_id AND id=NEW.id;
 SELECT * INTO o FROM archava_commerce.orders WHERE tenant_id=t.tenant_id AND id=t.order_id;
 SELECT count(*) INTO entries FROM archava_foodservice.ticket_lines WHERE tenant_id=t.tenant_id AND ticket_id=t.id;
 IF entries NOT BETWEEN 1 AND 20 OR o.managed_by IS DISTINCT FROM 'foodservice' OR o.owner_id IS DISTINCT FROM t.owner_id OR entries<>(SELECT count(*) FROM archava_commerce.order_lines WHERE tenant_id=t.tenant_id AND order_id=t.order_id) OR (t.state='draft' AND o.state<>'draft') OR (t.state='queued' AND o.state<>'reserved') OR (t.state IN ('preparing','ready','served','closed') AND o.state<>'fulfilled') OR (t.state='cancelled' AND o.state IS DISTINCT FROM CASE WHEN t.prepared_at IS NULL THEN 'cancelled' ELSE 'fulfilled' END)
 THEN RAISE EXCEPTION 'Ticket/order lifecycle binding refused' USING ERRCODE='23514';END IF;
 IF t.state NOT IN ('draft','cancelled') AND EXISTS(
  WITH expected AS (SELECT c.product_id,sum(c.quantity::bigint*l.quantity) AS quantity FROM archava_foodservice.ticket_lines l JOIN archava_foodservice.recipe_components c ON c.tenant_id=l.tenant_id AND c.menu_id=l.menu_id AND c.version=l.recipe_version WHERE l.tenant_id=t.tenant_id AND l.ticket_id=t.id GROUP BY c.product_id),actual AS (SELECT product_id,quantity FROM archava_commerce.allocations WHERE tenant_id=t.tenant_id AND order_id=t.order_id)
  SELECT 1 FROM expected FULL OUTER JOIN actual USING(product_id) WHERE expected.quantity IS DISTINCT FROM actual.quantity)
 THEN RAISE EXCEPTION 'Ticket recipe allocation binding refused' USING ERRCODE='23514';END IF;RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER ticket_complete AFTER INSERT OR UPDATE ON archava_foodservice.tickets DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_foodservice.assert_ticket();
CREATE FUNCTION archava_foodservice.guard_command() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF OLD.receipt IS NOT NULL OR TG_OP='DELETE' OR NEW.tenant_id IS DISTINCT FROM OLD.tenant_id OR NEW.request_id IS DISTINCT FROM OLD.request_id OR NEW.actor_id IS DISTINCT FROM OLD.actor_id OR NEW.fingerprint IS DISTINCT FROM OLD.fingerprint OR NEW.created_at IS DISTINCT FROM OLD.created_at THEN RAISE EXCEPTION 'Food-service command is immutable';END IF;RETURN NEW;
END $$;
CREATE TRIGGER command_guard BEFORE UPDATE OR DELETE ON archava_foodservice.commands FOR EACH ROW EXECUTE FUNCTION archava_foodservice.guard_command();
DO $$ DECLARE owned text;BEGIN
 FOREACH owned IN ARRAY ARRAY['memberships','tables','menu_items','recipe_versions','recipe_components','tickets','ticket_lines','commands','audit'] LOOP
  EXECUTE format('ALTER TABLE archava_foodservice.%I ENABLE ROW LEVEL SECURITY',owned);
  EXECUTE format('ALTER TABLE archava_foodservice.%I FORCE ROW LEVEL SECURITY',owned);
  EXECUTE format('CREATE POLICY food_tenant ON archava_foodservice.%I USING(tenant_id=current_setting(''archava.tenant_id'',true)) WITH CHECK(tenant_id=current_setting(''archava.tenant_id'',true))',owned);
 END LOOP;
END $$;
CREATE FUNCTION archava_foodservice.lock_membership(wanted_tenant text,wanted_principal text) RETURNS SETOF archava_foodservice.memberships LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog AS $$
 SELECT * FROM archava_foodservice.memberships WHERE tenant_id=wanted_tenant AND principal_id=wanted_principal AND wanted_tenant=current_setting('archava.tenant_id',true) FOR SHARE
$$;
REVOKE ALL ON FUNCTION archava_foodservice.lock_membership(text,text) FROM PUBLIC;
