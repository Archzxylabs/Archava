-- Explicit versioned due terms for actual supported original business documents.
-- This changes neither source amounts nor financial recognition/payment state.
DO $$ BEGIN
 PERFORM pg_advisory_xact_lock(hashtext('archava-serving-writer-takeover'));
 IF to_regclass('archava_control.writer_lease') IS NOT NULL THEN
  PERFORM 1 FROM archava_control.writer_lease WHERE lease_name='archava-serving' FOR UPDATE;
 END IF;
END $$;
ALTER TABLE archava_accounting.memberships DROP CONSTRAINT memberships_permissions_check;
ALTER TABLE archava_accounting.memberships ADD CONSTRAINT memberships_permissions_check CHECK(permissions <@ ARRAY[
 'books.read','books.write','accounts.write','journals.write','journals.post','periods.close','periods.reopen','reports.read','audit.read',
 'bank.read','bank.configure','bank.import','bank.reconcile','payables.read','payables.write','clearing.read','clearing.write','terms.read','terms.write']::text[]);
CREATE TABLE archava_accounting.document_terms(
 tenant_id text NOT NULL,id uuid NOT NULL,book_id uuid NOT NULL,request_id uuid NOT NULL,
 document_kind text NOT NULL CHECK(document_kind IN ('receivable','payable')),
 source_id uuid NOT NULL,document_id uuid NOT NULL,document_revision integer NOT NULL CHECK(document_revision>0),
 revision integer NOT NULL CHECK(revision>0),effective_from date NOT NULL CHECK(effective_from BETWEEN DATE '2000-01-01' AND DATE '2100-12-31'),
 due_kind text NOT NULL CHECK(due_kind IN ('unknown','due_on')),due_date date CHECK(due_date BETWEEN DATE '2000-01-01' AND DATE '2100-12-31'),
 created_by text NOT NULL,reason text NOT NULL CHECK(reason=btrim(reason) AND char_length(reason) BETWEEN 1 AND 2000 AND reason!~'[[:cntrl:]]'),
 recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 CHECK((due_kind='unknown' AND due_date IS NULL) OR (due_kind='due_on' AND due_date IS NOT NULL)),
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,book_id,id),UNIQUE(tenant_id,request_id),
 UNIQUE(tenant_id,document_kind,source_id,revision),UNIQUE(tenant_id,document_kind,document_id,revision),
 FOREIGN KEY(tenant_id,book_id) REFERENCES archava_accounting.books(tenant_id,id),
 FOREIGN KEY(tenant_id,created_by) REFERENCES archava_accounting.memberships(tenant_id,principal_id),
 FOREIGN KEY(tenant_id,request_id) REFERENCES archava_accounting.commands(tenant_id,request_id) DEFERRABLE INITIALLY DEFERRED
);
CREATE INDEX document_terms_cutoff ON archava_accounting.document_terms(tenant_id,book_id,document_kind,document_id,effective_from DESC,revision DESC);
ALTER TABLE archava_accounting.document_terms ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_accounting.document_terms FORCE ROW LEVEL SECURITY;
CREATE POLICY terms_tenant ON archava_accounting.document_terms USING(tenant_id=current_setting('archava.tenant_id',true)) WITH CHECK(tenant_id=current_setting('archava.tenant_id',true));
CREATE FUNCTION archava_accounting.terms_snapshot(s archava_accounting.document_terms)
 RETURNS jsonb LANGUAGE sql IMMUTABLE SET search_path=pg_catalog AS $$
 SELECT jsonb_build_object('documentKind',s.document_kind,'sourceId',s.source_id,'documentId',s.document_id,
 'documentRevision',s.document_revision,'termsRevision',s.revision,'effectiveFrom',s.effective_from,
 'condition',CASE WHEN s.due_kind='unknown' THEN jsonb_build_object('kind','unknown') ELSE jsonb_build_object('kind','due_on','date',s.due_date) END)
$$;
CREATE FUNCTION archava_accounting.guard_document_terms() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE b archava_accounting.books;m archava_accounting.memberships;previous archava_accounting.document_terms;
 original_document uuid;original_date date;observed_revision integer;
BEGIN
 SELECT * INTO b FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 SELECT * INTO m FROM archava_accounting.memberships WHERE tenant_id=NEW.tenant_id AND principal_id=NEW.created_by;
 IF b.id IS NULL OR b.source_authority<>'native' OR m.principal_id IS NULL OR NOT m.active OR m.kind<>'human'
 OR NOT m.permissions @> ARRAY['books.read','terms.read','terms.write']::text[] OR (m.scope<>'all' AND b.owner_id<>NEW.created_by)
 OR (NEW.document_kind='payable' AND NOT m.permissions @> ARRAY['payables.read']::text[]) THEN
  RAISE EXCEPTION 'Document terms require current native book and explicit human authority' USING ERRCODE='23514';END IF;
 IF NEW.document_kind='receivable' THEN
  SELECT s.invoice_id,s.source_date,d.revision INTO original_document,original_date,observed_revision
  FROM archava_accounting.invoice_sources s JOIN archava_billing.invoices d ON d.tenant_id=s.tenant_id AND d.id=s.invoice_id
  WHERE s.tenant_id=NEW.tenant_id AND s.book_id=NEW.book_id AND s.id=NEW.source_id AND s.effect='issue' AND d.company_id=b.company_id;
 ELSIF NEW.document_kind='payable' THEN
  SELECT s.document_id,s.source_date,d.revision INTO original_document,original_date,observed_revision
  FROM archava_accounting.purchase_sources s JOIN archava_purchasing.supplier_documents d ON d.tenant_id=s.tenant_id AND d.id=s.document_id
  WHERE s.tenant_id=NEW.tenant_id AND s.book_id=NEW.book_id AND s.id=NEW.source_id AND s.effect='bill' AND d.company_id=b.company_id;
 ELSE
  RAISE EXCEPTION 'Unsupported document terms source kind' USING ERRCODE='23514';
 END IF;
 IF original_document IS NULL OR NEW.document_id IS DISTINCT FROM original_document OR NEW.document_revision IS DISTINCT FROM observed_revision
 OR NEW.effective_from<original_date OR NOT EXISTS(SELECT 1 FROM archava_accounting.periods p WHERE p.tenant_id=NEW.tenant_id AND p.book_id=NEW.book_id AND p.state='open' AND NEW.effective_from BETWEEN p.start_date AND p.end_date) THEN
  RAISE EXCEPTION 'Document terms must retain the original source, current document revision and an open effective period' USING ERRCODE='23514';END IF;
 SELECT * INTO previous FROM archava_accounting.document_terms WHERE tenant_id=NEW.tenant_id AND document_kind=NEW.document_kind AND document_id=NEW.document_id ORDER BY revision DESC LIMIT 1;
 IF previous.id IS NULL THEN
  IF NEW.revision<>1 THEN RAISE EXCEPTION 'First document terms require revision one' USING ERRCODE='23514';END IF;
 ELSIF previous.source_id IS DISTINCT FROM NEW.source_id OR previous.book_id IS DISTINCT FROM NEW.book_id
 OR NEW.revision::bigint<>previous.revision::bigint+1 OR NEW.effective_from<previous.effective_from THEN
  RAISE EXCEPTION 'Document terms revisions must append without changing the source or backdating a later version' USING ERRCODE='23514';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER document_terms_guard BEFORE INSERT ON archava_accounting.document_terms FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_document_terms();
CREATE TRIGGER document_terms_immutable BEFORE UPDATE OR DELETE ON archava_accounting.document_terms FOR EACH ROW EXECUTE FUNCTION archava_accounting.immutable();
CREATE FUNCTION archava_accounting.assert_document_terms_claim() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE op text;r jsonb;actor text;expected text;
BEGIN
 expected:=CASE NEW.document_kind WHEN 'receivable' THEN 'invoice_terms.set' ELSE 'supplier_terms.set' END;
 SELECT a.operation,c.receipt,a.actor_id INTO op,r,actor FROM archava_accounting.audit a JOIN archava_accounting.commands c ON c.tenant_id=a.tenant_id AND c.request_id=a.request_id
 WHERE a.tenant_id=NEW.tenant_id AND a.request_id=NEW.request_id AND a.resource_kind='document_terms' AND a.resource_id=NEW.id AND a.revision=NEW.revision;
 IF op IS DISTINCT FROM expected OR r->>'type' IS DISTINCT FROM expected OR actor IS DISTINCT FROM NEW.created_by
 OR r->>'resourceKind' IS DISTINCT FROM 'document_terms' OR r->>'resourceId' IS DISTINCT FROM NEW.id::text OR r->>'bookId' IS DISTINCT FROM NEW.book_id::text
 OR r->>'sourceId' IS DISTINCT FROM NEW.source_id::text OR r->'documentTerms' IS DISTINCT FROM archava_accounting.terms_snapshot(NEW)
 OR (r->>'revision')::integer IS DISTINCT FROM NEW.revision THEN
  RAISE EXCEPTION 'Document terms require the exact reviewed immutable command receipt and audit claim' USING ERRCODE='23514';END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER document_terms_claim_complete AFTER INSERT ON archava_accounting.document_terms DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_accounting.assert_document_terms_claim();
CREATE FUNCTION archava_accounting.terms_guards_complete() RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog AS $$
 SELECT NOT EXISTS(SELECT 1 FROM (VALUES
 ('document_terms_guard','archava_accounting.guard_document_terms()',7,false,false),
 ('document_terms_immutable','archava_accounting.immutable()',27,false,false),
 ('document_terms_claim_complete','archava_accounting.assert_document_terms_claim()',5,true,true)
 ) AS expected(name,fn,kind,deferred_flag,initially_flag)
 WHERE NOT EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid='archava_accounting.document_terms'::regclass AND t.tgname=expected.name AND t.tgfoid=to_regprocedure(expected.fn)
 AND t.tgtype=expected.kind AND t.tgdeferrable=expected.deferred_flag AND t.tginitdeferred=expected.initially_flag AND t.tgnargs=0
 AND t.tgenabled='O' AND NOT t.tgisinternal AND t.tgqual IS NULL AND t.tgattr=''::int2vector))
 AND EXISTS(SELECT 1 FROM pg_class WHERE oid='archava_accounting.document_terms'::regclass AND relrowsecurity AND relforcerowsecurity)
$$;
REVOKE ALL ON FUNCTION archava_accounting.terms_guards_complete() FROM PUBLIC;
DO $$ BEGIN IF to_regprocedure('archava_control.install_runtime_write_guards()') IS NOT NULL THEN PERFORM archava_control.install_runtime_write_guards();END IF;END $$;
