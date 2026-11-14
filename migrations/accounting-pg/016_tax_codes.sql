-- Tax stage A: a company's own tax codes. A code belongs to one native book and
-- has a kind (output for sales, input for purchases), a rate and a base fraction
-- as integer fractions, a control account and an effective date range. One person
-- enters a version and a reviewer confirms it before it applies. Nothing
-- reads a code yet: no order, invoice, supplier document, posting policy or
-- report uses it. No row is created, no rate is shipped and no existing
-- membership gains a permission. Earlier SQL bytes are unchanged.
DO $$ BEGIN
 PERFORM pg_advisory_xact_lock(hashtext('archava-serving-writer-takeover'));
 IF to_regclass('archava_control.writer_lease') IS NOT NULL THEN
  PERFORM 1 FROM archava_control.writer_lease WHERE lease_name='archava-serving' FOR UPDATE;
 END IF;
END $$;
ALTER TABLE archava_accounting.memberships DROP CONSTRAINT memberships_permissions_check;
ALTER TABLE archava_accounting.memberships ADD CONSTRAINT memberships_permissions_check CHECK(permissions <@ ARRAY[
 'books.read','books.write','accounts.write','journals.write','journals.post','periods.close','periods.reopen','reports.read','audit.read',
 'bank.read','bank.configure','bank.import','bank.reconcile','payables.read','payables.write','clearing.read','clearing.write','terms.read','terms.write',
 'tax.read','tax.manage']::text[]);
ALTER TABLE archava_accounting.audit DROP CONSTRAINT audit_resource_kind_check;
ALTER TABLE archava_accounting.audit ADD CONSTRAINT audit_resource_kind_check CHECK(resource_kind IN
 ('book','account','period','journal','invoice_policy','purchase_policy','stock_policy','bank_account','bank_statement','bank_allocation','bank_unmatch','bank_policy','bank_reconciliation','document_terms','tax_code'));
-- One row is one version of a code. A version is `entered` when created and does
-- not apply until a reviewer confirms it. effective_to is the last day a confirmed
-- version applies; NULL means open-ended. A stored row accepts three changes only:
-- entered -> confirmed, entered -> withdrawn (it never applied) and
-- confirmed -> retired (its range is closed once).
CREATE TABLE archava_accounting.tax_codes(
 tenant_id text NOT NULL,id uuid NOT NULL,book_id uuid NOT NULL,
 code text NOT NULL CHECK(code ~ '^[A-Z0-9][A-Z0-9_-]{0,39}$'),version integer NOT NULL CHECK(version>0),
 kind text NOT NULL CHECK(kind IN ('output','input')),
 rate_numerator integer NOT NULL,rate_denominator integer NOT NULL,
 base_numerator integer NOT NULL,base_denominator integer NOT NULL,
 CHECK(rate_denominator BETWEEN 1 AND 1000000 AND rate_numerator BETWEEN 0 AND rate_denominator),
 CHECK(base_denominator BETWEEN 1 AND 1000000 AND base_numerator BETWEEN 1 AND base_denominator),
 control_account_id uuid NOT NULL,control_account_revision integer NOT NULL CHECK(control_account_revision>0),
 effective_from date NOT NULL CHECK(effective_from BETWEEN DATE '2000-01-01' AND DATE '2100-12-31'),
 effective_to date CHECK(effective_to BETWEEN effective_from AND DATE '2100-12-31'),
 state text NOT NULL DEFAULT 'entered' CHECK(state IN ('entered','confirmed','withdrawn','retired')),
 revision integer NOT NULL DEFAULT 1 CHECK(revision BETWEEN 1 AND 3),
 created_by text NOT NULL,reviewed_by text,
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),confirmed_at timestamptz,withdrawn_at timestamptz,retired_at timestamptz,
 CHECK((reviewed_by IS NULL)=(confirmed_at IS NULL) AND (effective_to IS NULL)=(retired_at IS NULL)),
 CHECK(CASE state
  WHEN 'entered' THEN revision=1 AND reviewed_by IS NULL AND withdrawn_at IS NULL AND retired_at IS NULL
  WHEN 'confirmed' THEN revision=2 AND reviewed_by IS NOT NULL AND withdrawn_at IS NULL AND retired_at IS NULL
  WHEN 'withdrawn' THEN revision=2 AND reviewed_by IS NULL AND withdrawn_at IS NOT NULL AND retired_at IS NULL
  ELSE revision=3 AND reviewed_by IS NOT NULL AND withdrawn_at IS NULL AND retired_at IS NOT NULL END),
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,book_id,id),UNIQUE(tenant_id,book_id,code,version),
 FOREIGN KEY(tenant_id,book_id) REFERENCES archava_accounting.books(tenant_id,id),
 FOREIGN KEY(tenant_id,book_id,control_account_id) REFERENCES archava_accounting.accounts(tenant_id,book_id,id),
 FOREIGN KEY(tenant_id,created_by) REFERENCES archava_accounting.memberships(tenant_id,principal_id),
 FOREIGN KEY(tenant_id,reviewed_by) REFERENCES archava_accounting.memberships(tenant_id,principal_id)
);
CREATE INDEX tax_code_range ON archava_accounting.tax_codes(tenant_id,book_id,code,effective_from);
ALTER TABLE archava_accounting.tax_codes ENABLE ROW LEVEL SECURITY;
ALTER TABLE archava_accounting.tax_codes FORCE ROW LEVEL SECURITY;
CREATE POLICY tax_code_tenant ON archava_accounting.tax_codes
 USING(tenant_id=current_setting('archava.tenant_id',true)) WITH CHECK(tenant_id=current_setting('archava.tenant_id',true));

-- A person named on a tax code is a current human member who may read and
-- manage tax codes of this book.
CREATE FUNCTION archava_accounting.tax_member_authorized(b archava_accounting.books,wanted_principal text)
 RETURNS boolean LANGUAGE sql STABLE SET search_path=pg_catalog AS $$
 SELECT EXISTS(SELECT 1 FROM archava_accounting.memberships m WHERE m.tenant_id=b.tenant_id AND m.principal_id=wanted_principal
  AND m.active AND m.kind='human' AND m.permissions @> ARRAY['books.read','tax.read','tax.manage']::text[]
  AND (m.scope='all' OR b.owner_id=wanted_principal))
$$;

-- Every insert and update first locks the book row FOR UPDATE, so two writers
-- of one book serialise and the second one's overlap and version reads (fresh
-- READ COMMITTED snapshots taken after the lock) see the first one's row.
CREATE FUNCTION archava_accounting.guard_tax_code() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE b archava_accounting.books;a archava_accounting.accounts;changed text[];
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Tax code history cannot be deleted' USING ERRCODE='23514';END IF;
 SELECT * INTO b FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 IF b.id IS NULL OR b.source_authority<>'native' THEN
  RAISE EXCEPTION 'A tax code requires an existing native book' USING ERRCODE='23514';END IF;
 IF TG_OP='UPDATE' THEN
  IF NEW.state='confirmed' AND OLD.state<>'entered' AND NEW.confirmed_at IS DISTINCT FROM OLD.confirmed_at THEN
   RAISE EXCEPTION 'Only an entered tax code can be confirmed' USING ERRCODE='23514';END IF;
  IF OLD.state IN ('withdrawn','retired') THEN
   RAISE EXCEPTION 'A withdrawn or retired tax code cannot change again' USING ERRCODE='23514';END IF;
  IF OLD.state='confirmed' AND NEW.state='retired' AND NEW.effective_to IS NULL THEN
   RAISE EXCEPTION 'Retiring a confirmed tax code needs its last day' USING ERRCODE='23514';END IF;
  changed:=CASE WHEN OLD.state='entered' AND NEW.state='confirmed' THEN ARRAY['state','revision','reviewed_by','confirmed_at']
   WHEN OLD.state='entered' AND NEW.state='withdrawn' THEN ARRAY['state','revision','withdrawn_at']
   WHEN OLD.state='confirmed' AND NEW.state='retired' THEN ARRAY['state','revision','effective_to','retired_at'] END;
  IF changed IS NULL OR (to_jsonb(NEW)-changed) IS DISTINCT FROM (to_jsonb(OLD)-changed) OR NEW.revision IS DISTINCT FROM OLD.revision+1
  OR (NEW.state='confirmed' AND (NEW.reviewed_by IS NULL OR NEW.confirmed_at IS NULL))
  OR (NEW.state='withdrawn' AND NEW.withdrawn_at IS NULL) OR (NEW.state='retired' AND NEW.retired_at IS NULL) THEN
   RAISE EXCEPTION 'A stored tax code is immutable; it can only be confirmed, withdrawn or retired' USING ERRCODE='23514';END IF;
  IF NEW.state='retired' AND NEW.effective_to<NEW.effective_from THEN
   RAISE EXCEPTION 'A tax code cannot end before it starts' USING ERRCODE='23514';END IF;
  IF NEW.state='confirmed' THEN
   IF NOT archava_accounting.tax_member_authorized(b,NEW.reviewed_by) THEN
    RAISE EXCEPTION 'A tax code requires a current human member with tax authority over the book' USING ERRCODE='23514';END IF;
   -- The same second-person rule as journal posting (001 guard_journal).
   IF b.independent_posting AND NEW.reviewed_by=NEW.created_by THEN
    RAISE EXCEPTION 'A book with independent posting needs a tax code reviewer other than the person who entered it' USING ERRCODE='23514';END IF;
   SELECT * INTO a FROM archava_accounting.accounts WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND id=NEW.control_account_id FOR SHARE;
   IF a.id IS NULL OR NOT a.active OR a.revision<>NEW.control_account_revision THEN
    RAISE EXCEPTION 'A tax code needs an active control account at its reviewed revision' USING ERRCODE='23514';END IF;
  END IF;
  RETURN NEW;
 END IF;
 IF NEW.state IS DISTINCT FROM 'entered' OR NEW.revision IS DISTINCT FROM 1 OR NEW.reviewed_by IS NOT NULL OR NEW.confirmed_at IS NOT NULL
 OR NEW.effective_to IS NOT NULL OR NEW.retired_at IS NOT NULL OR NEW.withdrawn_at IS NOT NULL THEN
  RAISE EXCEPTION 'A tax code starts entered, unconfirmed and open-ended at revision one' USING ERRCODE='23514';END IF;
 IF (NEW.rate_denominator BETWEEN 1 AND 1000000 AND NEW.rate_numerator BETWEEN 0 AND NEW.rate_denominator
  AND NEW.base_denominator BETWEEN 1 AND 1000000 AND NEW.base_numerator BETWEEN 1 AND NEW.base_denominator) IS NOT TRUE THEN
  RAISE EXCEPTION 'A tax rate must be between zero and one and its base fraction above zero and at most one' USING ERRCODE='23514';END IF;
 IF NOT archava_accounting.tax_member_authorized(b,NEW.created_by) THEN
  RAISE EXCEPTION 'A tax code requires a current human member with tax authority over the book' USING ERRCODE='23514';END IF;
 SELECT * INTO a FROM archava_accounting.accounts WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND id=NEW.control_account_id FOR SHARE;
 IF a.id IS NULL OR NOT a.active OR a.revision<>NEW.control_account_revision THEN
  RAISE EXCEPTION 'A tax code needs an active control account at its reviewed revision' USING ERRCODE='23514';END IF;
 IF a.class<>(CASE WHEN NEW.kind='output' THEN 'liability' ELSE 'asset' END) OR a.cash_role<>'operating' THEN
  RAISE EXCEPTION 'A tax control account must be an operating liability for output tax or an operating asset for input tax' USING ERRCODE='23514';END IF;
 -- A withdrawn version never applied: it fixes neither the kind nor a range.
 IF EXISTS(SELECT 1 FROM archava_accounting.tax_codes t WHERE t.tenant_id=NEW.tenant_id AND t.book_id=NEW.book_id AND t.code=NEW.code AND t.state<>'withdrawn' AND t.kind<>NEW.kind) THEN
  RAISE EXCEPTION 'A tax code keeps its kind across versions' USING ERRCODE='23514';END IF;
 IF NEW.version IS DISTINCT FROM (SELECT coalesce(max(t.version),0)+1 FROM archava_accounting.tax_codes t WHERE t.tenant_id=NEW.tenant_id AND t.book_id=NEW.book_id AND t.code=NEW.code) THEN
  RAISE EXCEPTION 'Tax code versions must append in order' USING ERRCODE='23514';END IF;
 IF EXISTS(SELECT 1 FROM archava_accounting.tax_codes t WHERE t.tenant_id=NEW.tenant_id AND t.book_id=NEW.book_id AND t.code=NEW.code AND t.state<>'withdrawn'
  AND coalesce(t.effective_to,DATE 'infinity')>=NEW.effective_from) THEN
  RAISE EXCEPTION 'Tax code date ranges of one code must not overlap' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER tax_code_guard BEFORE INSERT OR UPDATE OR DELETE ON archava_accounting.tax_codes FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_tax_code();
CREATE CONSTRAINT TRIGGER tax_code_complete AFTER INSERT OR UPDATE ON archava_accounting.tax_codes DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_accounting.assert_resource('tax_code');

-- The people stored on a row are the actors of the commands that changed it,
-- never a name taken from a request: the creator is the actor of
-- tax_code.create and the reviewer the actor of tax_code.confirm.
CREATE FUNCTION archava_accounting.assert_tax_code_claim() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE op text;actor text;wanted_op text;wanted_actor text;
BEGIN
 SELECT a.operation,a.actor_id INTO op,actor FROM archava_accounting.audit a
 WHERE a.tenant_id=NEW.tenant_id AND a.resource_kind='tax_code' AND a.resource_id=NEW.id AND a.revision=NEW.revision;
 IF TG_OP='INSERT' THEN wanted_op:='tax_code.create';wanted_actor:=NEW.created_by;
 ELSIF NEW.state='confirmed' THEN wanted_op:='tax_code.confirm';wanted_actor:=NEW.reviewed_by;
 ELSE wanted_op:='tax_code.retire';wanted_actor:=actor;END IF;
 IF op IS DISTINCT FROM wanted_op OR actor IS NULL OR actor IS DISTINCT FROM wanted_actor THEN
  RAISE EXCEPTION 'A tax code change must carry the reviewed command of the person it names' USING ERRCODE='23514';END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER tax_code_claim_complete AFTER INSERT OR UPDATE ON archava_accounting.tax_codes DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_accounting.assert_tax_code_claim();

CREATE FUNCTION archava_accounting.tax_guards_complete() RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog AS $$
 SELECT NOT EXISTS(SELECT 1 FROM (VALUES
 ('tax_code_guard','archava_accounting.guard_tax_code()',31,false,false,0,''),
 ('tax_code_complete','archava_accounting.assert_resource()',21,true,true,1,'7461785f636f646500'),
 ('tax_code_claim_complete','archava_accounting.assert_tax_code_claim()',21,true,true,0,'')
 ) AS expected(name,fn,kind,deferred_flag,initially_flag,args,argument_bytes)
 WHERE NOT EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid='archava_accounting.tax_codes'::regclass AND t.tgname=expected.name AND t.tgfoid=to_regprocedure(expected.fn)
 AND t.tgtype=expected.kind AND t.tgdeferrable=expected.deferred_flag AND t.tginitdeferred=expected.initially_flag AND t.tgnargs=expected.args AND encode(t.tgargs,'hex')=expected.argument_bytes
 AND t.tgenabled='O' AND NOT t.tgisinternal AND t.tgqual IS NULL AND t.tgattr=''::int2vector))
 AND EXISTS(SELECT 1 FROM pg_class WHERE oid='archava_accounting.tax_codes'::regclass AND relrowsecurity AND relforcerowsecurity)
$$;
REVOKE ALL ON FUNCTION archava_accounting.tax_guards_complete() FROM PUBLIC;
DO $$ BEGIN
 IF to_regprocedure('archava_control.install_runtime_write_guards()') IS NOT NULL THEN
  PERFORM archava_control.install_runtime_write_guards();
 END IF;
END $$;
