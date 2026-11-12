-- Operational supplier claims only. No accounting posting, tax or payment ledger.
ALTER TABLE archava_purchasing.memberships DROP CONSTRAINT memberships_permissions_check;
ALTER TABLE archava_purchasing.memberships ADD CONSTRAINT memberships_permissions_check CHECK(permissions <@ ARRAY['suppliers.read','suppliers.write','offers.read','offers.write','orders.read','orders.write','receipts.read','receipts.write','documents.read','documents.write','audit.read']::text[]);
CREATE TABLE archava_purchasing.supplier_documents (
 tenant_id text NOT NULL,id uuid NOT NULL,order_id uuid NOT NULL,owner_id text NOT NULL,supplier_id uuid NOT NULL,supplier_code text NOT NULL,supplier_name text NOT NULL,
 company_id uuid NOT NULL,company_revision integer NOT NULL,company_code text NOT NULL,company_name text NOT NULL,
 vendor_reference text NOT NULL CHECK(vendor_reference ~ '^[A-Z0-9][A-Z0-9 ./_-]{0,119}$' AND vendor_reference=btrim(vendor_reference) AND strpos(vendor_reference,'  ')=0),
 document_date date NOT NULL CHECK(document_date BETWEEN DATE '1900-01-01' AND DATE '9999-12-31'),currency text NOT NULL CHECK(currency='IDR'),declared_total bigint NOT NULL CHECK(declared_total BETWEEN 0 AND 500000000000000),claim_lines jsonb NOT NULL,
 state text NOT NULL CHECK(state IN ('captured','held','matched','void')),observed_order_revision integer NOT NULL CHECK(observed_order_revision>0),match_evidence jsonb NOT NULL DEFAULT '{}'::jsonb CHECK(jsonb_typeof(match_evidence)='object'),
 revision integer NOT NULL DEFAULT 1 CHECK(revision>0),created_at timestamptz NOT NULL DEFAULT clock_timestamp(),updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,company_id,supplier_id,vendor_reference),FOREIGN KEY(tenant_id,order_id) REFERENCES archava_purchasing.orders(tenant_id,id),FOREIGN KEY(tenant_id,company_id,company_revision) REFERENCES archava_organizations.company_audit(tenant_id,company_id,revision)
);
CREATE TABLE archava_purchasing.supplier_document_versions (
 tenant_id text NOT NULL,document_id uuid NOT NULL,revision integer NOT NULL CHECK(revision>0),request_id uuid NOT NULL,snapshot jsonb NOT NULL,
 PRIMARY KEY(tenant_id,document_id,revision),UNIQUE(tenant_id,request_id),FOREIGN KEY(tenant_id,document_id) REFERENCES archava_purchasing.supplier_documents(tenant_id,id),FOREIGN KEY(tenant_id,request_id) REFERENCES archava_purchasing.commands(tenant_id,request_id)
);
CREATE TABLE archava_purchasing.supplier_document_allocations (
 tenant_id text NOT NULL,document_id uuid NOT NULL,document_revision integer NOT NULL,order_id uuid NOT NULL,receipt_request_id uuid NOT NULL,line_id uuid NOT NULL,quantity integer NOT NULL CHECK(quantity BETWEEN 1 AND 1000000),released_revision integer CHECK(released_revision>document_revision),
 PRIMARY KEY(tenant_id,document_id,document_revision,receipt_request_id,line_id),
 FOREIGN KEY(tenant_id,document_id,document_revision) REFERENCES archava_purchasing.supplier_document_versions(tenant_id,document_id,revision) DEFERRABLE INITIALLY DEFERRED,
 FOREIGN KEY(tenant_id,document_id,released_revision) REFERENCES archava_purchasing.supplier_document_versions(tenant_id,document_id,revision) DEFERRABLE INITIALLY DEFERRED,
 FOREIGN KEY(tenant_id,receipt_request_id,line_id) REFERENCES archava_purchasing.receipt_lines(tenant_id,request_id,line_id)
);
CREATE INDEX supplier_documents_scope ON archava_purchasing.supplier_documents(tenant_id,company_id,owner_id,id);
CREATE INDEX supplier_receipt_lookup ON archava_purchasing.receipt_lines(tenant_id,order_id,line_id,request_id);
CREATE INDEX supplier_document_order_capacity ON archava_purchasing.supplier_document_allocations(tenant_id,order_id,line_id) WHERE released_revision IS NULL;
CREATE INDEX supplier_document_capacity ON archava_purchasing.supplier_document_allocations(tenant_id,receipt_request_id,line_id) WHERE released_revision IS NULL;
CREATE FUNCTION archava_purchasing.guard_supplier_document() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE source archava_purchasing.orders;claim jsonb;seen uuid[]='{}';line_id uuid;
BEGIN
 SELECT * INTO source FROM archava_purchasing.orders WHERE tenant_id=NEW.tenant_id AND id=NEW.order_id FOR UPDATE;
 IF source.id IS NULL OR source.confirmed_at IS NULL OR source.company_id IS NULL OR (NEW.owner_id,NEW.supplier_id,NEW.supplier_code,NEW.supplier_name,NEW.company_id,NEW.company_revision,NEW.company_code,NEW.company_name,NEW.currency) IS DISTINCT FROM (source.owner_id,source.supplier_id,source.supplier_code,source.supplier_name,source.company_id,source.company_revision,source.company_code,source.company_name,source.currency) OR NEW.observed_order_revision>source.revision THEN RAISE EXCEPTION 'Supplier document source identity refused' USING ERRCODE='23514';END IF;
 IF TG_OP='INSERT' THEN
  IF NEW.revision<>1 OR NEW.state<>'captured' OR NEW.match_evidence<>'{}'::jsonb THEN RAISE EXCEPTION 'Initial supplier document refused' USING ERRCODE='23514';END IF;
 ELSE
  IF (NEW.tenant_id,NEW.id,NEW.order_id,NEW.owner_id,NEW.supplier_id,NEW.supplier_code,NEW.supplier_name,NEW.company_id,NEW.company_revision,NEW.company_code,NEW.company_name,NEW.vendor_reference,NEW.currency,NEW.created_at) IS DISTINCT FROM (OLD.tenant_id,OLD.id,OLD.order_id,OLD.owner_id,OLD.supplier_id,OLD.supplier_code,OLD.supplier_name,OLD.company_id,OLD.company_revision,OLD.company_code,OLD.company_name,OLD.vendor_reference,OLD.currency,OLD.created_at) OR NEW.revision<>OLD.revision+1 THEN RAISE EXCEPTION 'Supplier document identity is immutable' USING ERRCODE='23514';END IF;
  IF (OLD.state='matched' AND NEW.state<>'void') OR (OLD.state='void' AND NEW.state<>'captured') OR (NEW.state<>'captured' AND (NEW.document_date,NEW.declared_total,NEW.claim_lines) IS DISTINCT FROM (OLD.document_date,OLD.declared_total,OLD.claim_lines)) OR (NEW.state='captured' AND NEW.match_evidence<>'{}'::jsonb) THEN RAISE EXCEPTION 'Supplier document transition refused' USING ERRCODE='23514';END IF;
 END IF;
 IF jsonb_typeof(NEW.claim_lines)<>'array' OR jsonb_array_length(NEW.claim_lines) NOT BETWEEN 1 AND 50 THEN RAISE EXCEPTION 'Supplier claim lines refused' USING ERRCODE='23514';END IF;
 FOR claim IN SELECT value FROM jsonb_array_elements(NEW.claim_lines) LOOP
  line_id=(claim->>'lineId')::uuid;
  IF line_id IS NULL OR line_id=ANY(seen) OR (SELECT count(*) FROM jsonb_object_keys(claim))<>3 OR NOT (claim ?& ARRAY['lineId','quantity','unitCost']) OR jsonb_typeof(claim->'quantity') IS DISTINCT FROM 'number' OR jsonb_typeof(claim->'unitCost') IS DISTINCT FROM 'number' OR (claim->>'quantity')::bigint NOT BETWEEN 1 AND 1000000 OR (claim->>'unitCost')::bigint NOT BETWEEN 0 AND 1000000000 OR NOT EXISTS(SELECT 1 FROM archava_purchasing.lines WHERE tenant_id=NEW.tenant_id AND order_id=NEW.order_id AND id=line_id) THEN RAISE EXCEPTION 'Supplier claim line source refused' USING ERRCODE='23514';END IF;
  seen=array_append(seen,line_id);
 END LOOP;
 IF NEW.state='held' AND (jsonb_typeof(NEW.match_evidence->'reasons') IS DISTINCT FROM 'array' OR jsonb_array_length(NEW.match_evidence->'reasons')=0) THEN RAISE EXCEPTION 'Held supplier document requires reasons' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER supplier_document_guard BEFORE INSERT OR UPDATE ON archava_purchasing.supplier_documents FOR EACH ROW EXECUTE FUNCTION archava_purchasing.guard_supplier_document();
CREATE TRIGGER supplier_document_no_delete BEFORE DELETE ON archava_purchasing.supplier_documents FOR EACH ROW EXECUTE FUNCTION archava_purchasing.immutable_record();
CREATE TRIGGER supplier_document_version_immutable BEFORE UPDATE OR DELETE ON archava_purchasing.supplier_document_versions FOR EACH ROW EXECUTE FUNCTION archava_purchasing.immutable_record();
CREATE FUNCTION archava_purchasing.guard_supplier_document_allocation() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE doc archava_purchasing.supplier_documents;accepted integer;used bigint;receipt_order uuid;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Supplier allocation history is immutable' USING ERRCODE='23514';END IF;
 PERFORM 1 FROM archava_purchasing.orders WHERE tenant_id=NEW.tenant_id AND id=NEW.order_id FOR UPDATE;
 SELECT * INTO doc FROM archava_purchasing.supplier_documents WHERE tenant_id=NEW.tenant_id AND id=NEW.document_id;
 IF doc.id IS NULL OR doc.order_id<>NEW.order_id THEN RAISE EXCEPTION 'Supplier allocation source refused' USING ERRCODE='23514';END IF;
 IF TG_OP='UPDATE' THEN
  IF (NEW.tenant_id,NEW.document_id,NEW.document_revision,NEW.order_id,NEW.receipt_request_id,NEW.line_id,NEW.quantity) IS DISTINCT FROM (OLD.tenant_id,OLD.document_id,OLD.document_revision,OLD.order_id,OLD.receipt_request_id,OLD.line_id,OLD.quantity) OR OLD.released_revision IS NOT NULL OR NEW.released_revision IS DISTINCT FROM doc.revision OR doc.state<>'void' THEN RAISE EXCEPTION 'Supplier allocation release refused' USING ERRCODE='23514';END IF;
 ELSE
  IF NEW.document_revision<>doc.revision OR doc.state<>'matched' OR NEW.released_revision IS NOT NULL OR NOT EXISTS(SELECT 1 FROM jsonb_array_elements(doc.claim_lines) c WHERE (c->>'lineId')::uuid=NEW.line_id) THEN RAISE EXCEPTION 'Supplier allocation match refused' USING ERRCODE='23514';END IF;
 END IF;
 SELECT r.accepted,r.order_id INTO accepted,receipt_order FROM archava_purchasing.receipt_lines r WHERE r.tenant_id=NEW.tenant_id AND r.request_id=NEW.receipt_request_id AND r.line_id=NEW.line_id;
 SELECT coalesce(sum(a.quantity),0) INTO used FROM archava_purchasing.supplier_document_allocations a WHERE a.tenant_id=NEW.tenant_id AND a.receipt_request_id=NEW.receipt_request_id AND a.line_id=NEW.line_id AND a.released_revision IS NULL AND (a.document_id,a.document_revision,a.receipt_request_id,a.line_id) IS DISTINCT FROM (NEW.document_id,NEW.document_revision,NEW.receipt_request_id,NEW.line_id);
 IF accepted IS NULL OR receipt_order IS DISTINCT FROM NEW.order_id OR (NEW.released_revision IS NULL AND used+NEW.quantity>accepted) THEN RAISE EXCEPTION 'Accepted goods already allocated or rejected' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER supplier_document_allocation_guard BEFORE INSERT OR UPDATE OR DELETE ON archava_purchasing.supplier_document_allocations FOR EACH ROW EXECUTE FUNCTION archava_purchasing.guard_supplier_document_allocation();
CREATE FUNCTION archava_purchasing.assert_supplier_document() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE d archava_purchasing.supplier_documents;wanted_id uuid;claim jsonb;allocated bigint;cost integer;total bigint=0;v archava_purchasing.supplier_document_versions;
BEGIN
 IF TG_TABLE_NAME='supplier_documents' THEN wanted_id=NEW.id;ELSE wanted_id=NEW.document_id;END IF;
 SELECT * INTO d FROM archava_purchasing.supplier_documents WHERE tenant_id=NEW.tenant_id AND id=wanted_id;
 IF TG_TABLE_NAME='supplier_documents' THEN
  SELECT * INTO v FROM archava_purchasing.supplier_document_versions WHERE tenant_id=NEW.tenant_id AND document_id=NEW.id AND revision=NEW.revision;
  IF v.document_id IS NULL OR v.snapshot IS DISTINCT FROM to_jsonb(NEW) OR NOT EXISTS(SELECT 1 FROM archava_purchasing.audit a WHERE a.tenant_id=NEW.tenant_id AND a.request_id=v.request_id AND a.record_id=NEW.id AND a.revision=NEW.revision AND a.operation=CASE WHEN NEW.state='captured' THEN CASE WHEN NEW.revision=1 THEN 'document.capture' ELSE 'document.revise' END WHEN NEW.state='void' THEN 'document.void' ELSE 'document.match' END) THEN RAISE EXCEPTION 'Supplier document version/audit missing' USING ERRCODE='23514';END IF;
 END IF;
 IF d.state='matched' THEN
  FOR claim IN SELECT value FROM jsonb_array_elements(d.claim_lines) LOOP
   SELECT coalesce(sum(quantity),0) INTO allocated FROM archava_purchasing.supplier_document_allocations WHERE tenant_id=d.tenant_id AND document_id=d.id AND document_revision=d.revision AND line_id=(claim->>'lineId')::uuid AND released_revision IS NULL;
   SELECT unit_cost INTO cost FROM archava_purchasing.lines WHERE tenant_id=d.tenant_id AND order_id=d.order_id AND id=(claim->>'lineId')::uuid;
   IF allocated<>(claim->>'quantity')::bigint OR cost IS DISTINCT FROM (claim->>'unitCost')::integer THEN RAISE EXCEPTION 'Supplier match is not exact' USING ERRCODE='23514';END IF;
   total=total+(claim->>'quantity')::bigint*(claim->>'unitCost')::bigint;
  END LOOP;
  IF total<>d.declared_total OR (d.match_evidence->'reasons') IS DISTINCT FROM '[]'::jsonb THEN RAISE EXCEPTION 'Supplier matched total refused' USING ERRCODE='23514';END IF;
 ELSIF EXISTS(SELECT 1 FROM archava_purchasing.supplier_document_allocations WHERE tenant_id=d.tenant_id AND document_id=d.id AND released_revision IS NULL) THEN RAISE EXCEPTION 'Unmatched supplier document holds accepted goods' USING ERRCODE='23514';END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER supplier_document_complete AFTER INSERT OR UPDATE ON archava_purchasing.supplier_documents DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_purchasing.assert_supplier_document();
CREATE CONSTRAINT TRIGGER supplier_allocation_complete AFTER INSERT OR UPDATE ON archava_purchasing.supplier_document_allocations DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_purchasing.assert_supplier_document();
CREATE CONSTRAINT TRIGGER supplier_document_revision_complete AFTER INSERT OR UPDATE ON archava_purchasing.supplier_documents DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_purchasing.assert_entity();
DO $$ DECLARE owned text;BEGIN FOREACH owned IN ARRAY ARRAY['supplier_documents','supplier_document_versions','supplier_document_allocations'] LOOP
 EXECUTE format('ALTER TABLE archava_purchasing.%I ENABLE ROW LEVEL SECURITY',owned);EXECUTE format('ALTER TABLE archava_purchasing.%I FORCE ROW LEVEL SECURITY',owned);
 EXECUTE format('CREATE POLICY tenant_scope ON archava_purchasing.%I USING (tenant_id=current_setting(''archava.tenant_id'',true)) WITH CHECK (tenant_id=current_setting(''archava.tenant_id'',true))',owned);
END LOOP;END $$;

CREATE FUNCTION archava_purchasing.assert_supplier_document_version() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF NOT EXISTS(SELECT 1 FROM archava_purchasing.supplier_documents d JOIN archava_purchasing.audit a ON a.tenant_id=d.tenant_id AND a.record_id=d.id WHERE d.tenant_id=NEW.tenant_id AND d.id=NEW.document_id AND NEW.revision<=d.revision AND a.request_id=NEW.request_id AND a.revision=NEW.revision AND a.operation LIKE 'document.%') OR NEW.snapshot->>'id' IS DISTINCT FROM NEW.document_id::text OR (NEW.snapshot->>'revision')::integer IS DISTINCT FROM NEW.revision THEN RAISE EXCEPTION 'Supplier document history binding refused' USING ERRCODE='23514';END IF;RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER supplier_version_complete AFTER INSERT ON archava_purchasing.supplier_document_versions DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_purchasing.assert_supplier_document_version();
