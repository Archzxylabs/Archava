-- Tax review (2026-11-14), Accounting part. No table or column is added and no
-- row is rewritten. Earlier SQL bytes are unchanged: every existing function
-- below is replaced in place with its signature, so its OID, owner, grants and
-- the triggers that source_guards_complete(), purchase_guards_complete(),
-- stock_guards_complete() and tax_guards_complete() identify by OID are kept.
--
-- 1. A tax control account and a posting policy account exclude each other,
--    in both directions and at the source. A tax code is not created or
--    confirmed with a control account that the book's invoice, purchase or
--    stock posting policy names; a posting policy is not created or revised
--    to name the control account of a tax code version of its book that is
--    entered, confirmed or retired (a withdrawn version never applied and
--    does not count). Both sides lock the book row first, so they serialise.
--    Only inserts and updates are checked: a book that already holds such a
--    pair keeps it until one side is changed, and the capture guards below
--    still refuse to post a taxed document through it.
-- 2. The capture guards compare the policy mapping's account with the tax
--    control account as uuids. They compared text, so a stored mapping that
--    spells the id in upper case was not recognised as the same account.
-- 3. The reversal of a document is not reviewed before the document is
--    posted: the draft of a purchase void is refused while the bill source it
--    reverses has no posted journal, and the draft of an invoice correction
--    or void while the invoice's issue source carries an amount and has none.
--    A source journal cannot be reversed, so a posted original stays posted.
--    A reversal draft stored before this migration is not looked at again.
-- 4. tax_document_guards_complete(): readiness over the tax guards that live
--    outside Accounting (Billing 005/009/011, Commerce 012, Purchasing 008),
--    which Accounting, migrating after them, can name directly.
DO $$ BEGIN
 PERFORM pg_advisory_xact_lock(hashtext('archava-serving-writer-takeover'));
 IF to_regclass('archava_control.writer_lease') IS NOT NULL THEN
  PERFORM 1 FROM archava_control.writer_lease WHERE lease_name='archava-serving' FOR UPDATE;
 END IF;
END $$;

-- Whether an account is named by a current posting policy of the book: one of
-- the three mapped accounts of the invoice policy, one of the four of the
-- purchase policy, or the cost of goods sold account of the stock policy.
CREATE FUNCTION archava_accounting.posting_policy_account(wanted_tenant text,wanted_book uuid,wanted_account uuid)
 RETURNS boolean LANGUAGE sql STABLE SET search_path=pg_catalog AS $$
 SELECT EXISTS(SELECT 1 FROM archava_accounting.invoice_policies p CROSS JOIN LATERAL jsonb_array_elements(p.mappings) v
  WHERE p.tenant_id=wanted_tenant AND p.book_id=wanted_book AND (v->>'accountId')::uuid=wanted_account)
 OR EXISTS(SELECT 1 FROM archava_accounting.purchase_policies p CROSS JOIN LATERAL jsonb_array_elements(p.mappings) v
  WHERE p.tenant_id=wanted_tenant AND p.book_id=wanted_book AND (v->>'accountId')::uuid=wanted_account)
 OR EXISTS(SELECT 1 FROM archava_accounting.stock_policies p
  WHERE p.tenant_id=wanted_tenant AND p.book_id=wanted_book AND p.cogs_account_id=wanted_account)
$$;
-- Whether one of the accounts is the control account of a tax code version of
-- the book that is entered, confirmed or retired.
CREATE FUNCTION archava_accounting.tax_control_account(wanted_tenant text,wanted_book uuid,wanted_accounts uuid[])
 RETURNS boolean LANGUAGE sql STABLE SET search_path=pg_catalog AS $$
 SELECT EXISTS(SELECT 1 FROM archava_accounting.tax_codes t
  WHERE t.tenant_id=wanted_tenant AND t.book_id=wanted_book AND t.state<>'withdrawn' AND t.control_account_id=ANY(wanted_accounts))
$$;

-- The tax code guard of 016 with the policy check, at creation and again at
-- confirmation.
CREATE OR REPLACE FUNCTION archava_accounting.guard_tax_code() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
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
   IF archava_accounting.posting_policy_account(NEW.tenant_id,NEW.book_id,NEW.control_account_id) THEN
    RAISE EXCEPTION 'A tax control account cannot be an account of a posting policy of the book' USING ERRCODE='23514';END IF;
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
 IF archava_accounting.posting_policy_account(NEW.tenant_id,NEW.book_id,NEW.control_account_id) THEN
  RAISE EXCEPTION 'A tax control account cannot be an account of a posting policy of the book' USING ERRCODE='23514';END IF;
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

-- The policy guards of 014 with the tax code check, after every mapping has
-- been read as an account of the book.
CREATE OR REPLACE FUNCTION archava_accounting.guard_invoice_policy() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE b archava_accounting.books; m jsonb; a archava_accounting.accounts; wanted_class text;wanted_cash text;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Invoice posting policy history cannot be deleted' USING ERRCODE='23514';END IF;
 SELECT * INTO b FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 IF b.id IS NULL OR b.source_authority<>'native' OR b.currency_catalogue<>NEW.currency_catalogue
  OR ((NEW.source_unit='whole_idr' AND b.currency='IDR' AND b.minor_units=2) OR (NEW.source_unit='minor' AND b.currency<>'IDR')) IS NOT TRUE THEN
  RAISE EXCEPTION 'This source bridge requires a reviewed native book and its source unit: whole_idr on an IDR book, minor on any other' USING ERRCODE='23514';END IF;
 IF TG_OP='INSERT' THEN
  IF NEW.revision<>1 THEN RAISE EXCEPTION 'Initial policy revision refused' USING ERRCODE='23514';END IF;
 ELSE
  IF (to_jsonb(NEW)-ARRAY['revision','mappings']) IS DISTINCT FROM (to_jsonb(OLD)-ARRAY['revision','mappings']) OR NEW.revision<>OLD.revision+1 THEN
   RAISE EXCEPTION 'Policy identity and unit basis are immutable' USING ERRCODE='23514';END IF;
 END IF;
 IF (SELECT count(DISTINCT v->>'role') FROM jsonb_array_elements(NEW.mappings) v)<>3 OR
    (SELECT count(DISTINCT v->>'accountId') FROM jsonb_array_elements(NEW.mappings) v)<>3 THEN
  RAISE EXCEPTION 'Three distinct source roles/accounts are required' USING ERRCODE='23514';END IF;
 FOR m IN SELECT * FROM jsonb_array_elements(NEW.mappings) LOOP
  IF m->>'role' NOT IN ('receivable','revenue','payment_clearing') OR NOT(m ?& ARRAY['role','accountId','expectedAccountRevision']) OR (m-ARRAY['role','accountId','expectedAccountRevision'])<>'{}'::jsonb THEN
   RAISE EXCEPTION 'Unknown source account mapping' USING ERRCODE='23514';END IF;
  wanted_class:=CASE WHEN m->>'role'='revenue' THEN 'revenue' ELSE 'asset' END;
  wanted_cash:=CASE WHEN m->>'role'='payment_clearing' THEN 'none' ELSE 'operating' END;
  SELECT * INTO a FROM archava_accounting.accounts WHERE tenant_id=NEW.tenant_id AND book_id=b.id AND id=(m->>'accountId')::uuid FOR SHARE;
  IF a.id IS NULL OR NOT a.active OR a.revision<>(m->>'expectedAccountRevision')::integer OR a.class<>wanted_class OR a.cash_role<>wanted_cash THEN
   RAISE EXCEPTION 'A source account differs from its reviewed classification/revision' USING ERRCODE='23514';END IF;
 END LOOP;
 IF archava_accounting.tax_control_account(NEW.tenant_id,NEW.book_id,ARRAY(SELECT (v->>'accountId')::uuid FROM jsonb_array_elements(NEW.mappings) v)) THEN
  RAISE EXCEPTION 'A posting policy account cannot be the control account of a tax code of the book' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE OR REPLACE FUNCTION archava_accounting.guard_purchase_policy() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE b archava_accounting.books; m jsonb; a archava_accounting.accounts; wanted_class text;wanted_cash text;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Purchase posting policy history cannot be deleted' USING ERRCODE='23514';END IF;
 SELECT * INTO b FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 IF b.id IS NULL OR b.source_authority<>'native' OR b.currency_catalogue<>NEW.currency_catalogue
  OR ((NEW.source_unit='whole_idr' AND b.currency='IDR' AND b.minor_units=2) OR (NEW.source_unit='minor' AND b.currency<>'IDR')) IS NOT TRUE THEN
  RAISE EXCEPTION 'This source bridge requires a reviewed native book and its source unit: whole_idr on an IDR book, minor on any other' USING ERRCODE='23514';END IF;
 IF TG_OP='INSERT' THEN
  IF NEW.revision<>1 THEN RAISE EXCEPTION 'Initial policy revision refused' USING ERRCODE='23514';END IF;
 ELSE
  IF (to_jsonb(NEW)-ARRAY['revision','mappings']) IS DISTINCT FROM (to_jsonb(OLD)-ARRAY['revision','mappings']) OR NEW.revision<>OLD.revision+1 THEN
   RAISE EXCEPTION 'Policy identity and unit basis are immutable' USING ERRCODE='23514';END IF;
 END IF;
 IF (SELECT count(DISTINCT v->>'role') FROM jsonb_array_elements(NEW.mappings) v)<>4 OR
    (SELECT count(DISTINCT v->>'accountId') FROM jsonb_array_elements(NEW.mappings) v)<>4 THEN
  RAISE EXCEPTION 'Four distinct source roles/accounts are required' USING ERRCODE='23514';END IF;
 FOR m IN SELECT * FROM jsonb_array_elements(NEW.mappings) LOOP
  IF m->>'role' NOT IN ('inventory','goods_received_not_invoiced','payable','landed_fee_accrual') OR NOT(m ?& ARRAY['role','accountId','expectedAccountRevision']) OR jsonb_typeof(m->'expectedAccountRevision') IS DISTINCT FROM 'number' OR (m-ARRAY['role','accountId','expectedAccountRevision'])<>'{}'::jsonb THEN
   RAISE EXCEPTION 'Unknown source account mapping' USING ERRCODE='23514';END IF;
  wanted_class:=CASE WHEN m->>'role'='inventory' THEN 'asset' ELSE 'liability' END;
  wanted_cash:='operating';
  SELECT * INTO a FROM archava_accounting.accounts WHERE tenant_id=NEW.tenant_id AND book_id=b.id AND id=(m->>'accountId')::uuid FOR SHARE;
  IF a.id IS NULL OR NOT a.active OR a.revision IS DISTINCT FROM (m->>'expectedAccountRevision')::integer OR a.class<>wanted_class OR a.cash_role<>wanted_cash THEN
   RAISE EXCEPTION 'A source account differs from its reviewed classification/revision' USING ERRCODE='23514';END IF;
 END LOOP;
 IF archava_accounting.tax_control_account(NEW.tenant_id,NEW.book_id,ARRAY(SELECT (v->>'accountId')::uuid FROM jsonb_array_elements(NEW.mappings) v)) THEN
  RAISE EXCEPTION 'A posting policy account cannot be the control account of a tax code of the book' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE OR REPLACE FUNCTION archava_accounting.guard_stock_policy() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE b archava_accounting.books;a archava_accounting.accounts;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Stock posting policy history cannot be deleted' USING ERRCODE='23514';END IF;
 SELECT * INTO b FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 SELECT * INTO a FROM archava_accounting.accounts WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND id=NEW.cogs_account_id FOR SHARE;
 IF b.id IS NULL OR b.source_authority<>'native' OR b.currency_catalogue<>NEW.currency_catalogue
  OR ((NEW.source_unit='whole_idr' AND b.currency='IDR' AND b.minor_units=2) OR (NEW.source_unit='minor' AND b.currency<>'IDR')) IS NOT TRUE
  OR a.id IS NULL OR NOT a.active OR a.revision<>NEW.account_revision OR a.class<>'cost_of_goods_sold' OR a.cash_role<>'operating' THEN RAISE EXCEPTION 'Stock policy requires a reviewed native book, its source unit (whole_idr on an IDR book, minor on any other) and a current COGS account' USING ERRCODE='23514';END IF;
 IF TG_OP='INSERT' THEN
  IF NEW.revision<>1 THEN RAISE EXCEPTION 'Initial stock policy revision refused' USING ERRCODE='23514';END IF;
 ELSIF (to_jsonb(NEW)-ARRAY['revision','cogs_account_id','account_revision']) IS DISTINCT FROM (to_jsonb(OLD)-ARRAY['revision','cogs_account_id','account_revision']) OR NEW.revision<>OLD.revision+1 THEN RAISE EXCEPTION 'Stock policy identity and unit basis are immutable' USING ERRCODE='23514';END IF;
 IF archava_accounting.tax_control_account(NEW.tenant_id,NEW.book_id,ARRAY[NEW.cogs_account_id]) THEN
  RAISE EXCEPTION 'A posting policy account cannot be the control account of a tax code of the book' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;

-- The invoice source guard of 017; only the comparison of the policy account
-- with the tax control account changes (uuid, not text).
CREATE OR REPLACE FUNCTION archava_accounting.guard_invoice_source() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE c archava_billing.commands;i archava_billing.invoices;b archava_accounting.books;expected record;held jsonb;original archava_accounting.invoice_sources;unit_factor numeric;
 tax_part numeric;t archava_accounting.tax_codes;a archava_accounting.accounts;
BEGIN
 SELECT * INTO b FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 SELECT * INTO c FROM archava_billing.commands WHERE tenant_id=NEW.tenant_id AND request_id=NEW.id;
 SELECT * INTO i FROM archava_billing.invoices WHERE tenant_id=NEW.tenant_id AND id=NEW.invoice_id;
 SELECT * INTO expected FROM archava_accounting.invoice_effect(NEW.tenant_id,NEW.id);
 SELECT snapshot->'record' INTO held FROM archava_accounting.audit WHERE tenant_id=NEW.tenant_id AND resource_kind='invoice_policy' AND resource_id=NEW.policy_id AND revision=NEW.policy_revision;
 unit_factor:=CASE WHEN held->>'source_unit'='whole_idr' AND b.currency='IDR' AND b.minor_units=2 THEN 100 WHEN held->>'source_unit'='minor' AND b.currency<>'IDR' THEN 1 END;
 IF b.id IS NULL OR b.source_authority<>'native' OR unit_factor IS NULL OR i.id IS NULL OR i.company_id IS DISTINCT FROM b.company_id OR i.currency IS DISTINCT FROM b.currency
  OR c.actor_id IS DISTINCT FROM NEW.actor_id OR c.receipt->>'invoiceId' IS DISTINCT FROM NEW.invoice_id::text OR c.receipt->>'revision' IS DISTINCT FROM NEW.invoice_revision::text
  OR c.receipt->>'executedAt' IS NULL OR (c.receipt->>'executedAt')::timestamptz IS DISTINCT FROM NEW.occurred_at OR NEW.source_date IS DISTINCT FROM (NEW.occurred_at AT TIME ZONE 'UTC')::date
  OR NEW.invoice_code IS DISTINCT FROM i.invoice_code OR expected.effect IS DISTINCT FROM NEW.effect OR expected.whole_idr*unit_factor IS DISTINCT FROM NEW.amount_minor::numeric
  OR held IS NULL OR held IS DISTINCT FROM NEW.policy_snapshot OR held->>'book_id' IS DISTINCT FROM NEW.book_id::text THEN
  RAISE EXCEPTION 'Financial source differs from its immutable invoice receipt/policy' USING ERRCODE='23514';END IF;
 IF NEW.effect<>'issue' THEN
  SELECT * INTO original FROM archava_accounting.invoice_sources WHERE tenant_id=NEW.tenant_id AND invoice_id=NEW.invoice_id AND effect='issue';
  IF original.id IS NULL OR (NEW.book_id,NEW.policy_id,NEW.policy_revision,NEW.policy_snapshot) IS DISTINCT FROM (original.book_id,original.policy_id,original.policy_revision,original.policy_snapshot) THEN
   RAISE EXCEPTION 'Invoice corrections retain their original posting policy' USING ERRCODE='23514';END IF;
 END IF;
 tax_part:=archava_accounting.invoice_effect_tax(NEW.tenant_id,NEW.id);
 IF tax_part IS NULL OR tax_part*unit_factor IS DISTINCT FROM NEW.tax_minor::numeric THEN
  RAISE EXCEPTION 'Financial source tax differs from the tax stored on its invoice' USING ERRCODE='23514';END IF;
 IF i.tax_code_id IS NULL OR NEW.effect IN ('collection','refund') THEN
  IF NEW.tax_account_id IS NOT NULL OR NEW.tax_account_revision IS NOT NULL THEN
   RAISE EXCEPTION 'Financial source tax differs from the tax stored on its invoice' USING ERRCODE='23514';END IF;
 ELSIF NEW.effect='issue' THEN
  SELECT * INTO t FROM archava_accounting.tax_codes WHERE tenant_id=NEW.tenant_id AND id=i.tax_code_id FOR SHARE;
  IF t.id IS NULL OR t.book_id IS DISTINCT FROM NEW.book_id OR t.kind<>'output' OR t.state NOT IN ('confirmed','retired') THEN
   RAISE EXCEPTION 'The invoice tax code is not a confirmed output tax code of the posting book' USING ERRCODE='23514';END IF;
  SELECT * INTO a FROM archava_accounting.accounts WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND id=t.control_account_id FOR SHARE;
  IF a.id IS NULL OR NOT a.active THEN
   RAISE EXCEPTION 'The tax control account of the invoice tax code is inactive' USING ERRCODE='23514';END IF;
  IF EXISTS(SELECT 1 FROM jsonb_array_elements(held->'mappings') v WHERE (v->>'accountId')::uuid=t.control_account_id) THEN
   RAISE EXCEPTION 'The tax control account is also an account of the invoice posting policy' USING ERRCODE='23514';END IF;
  IF (NEW.tax_account_id,NEW.tax_account_revision) IS DISTINCT FROM (a.id,a.revision) THEN
   RAISE EXCEPTION 'Financial source tax differs from the tax stored on its invoice' USING ERRCODE='23514';END IF;
 ELSIF (NEW.tax_account_id,NEW.tax_account_revision) IS DISTINCT FROM (original.tax_account_id,original.tax_account_revision) THEN
  RAISE EXCEPTION 'Invoice corrections retain their original posting policy' USING ERRCODE='23514';
 END IF;
 IF EXISTS(SELECT 1 FROM archava_accounting.periods WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND state='closed' AND NEW.source_date BETWEEN start_date AND end_date) THEN
  RAISE EXCEPTION 'Reopen the financial source period before changing its invoice' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;

-- The purchase source guard of 018; the same single change.
CREATE OR REPLACE FUNCTION archava_accounting.guard_purchase_source() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE expected jsonb;o archava_accounting.purchase_origins;b archava_accounting.books;
 part record;unit_factor numeric;t archava_accounting.tax_codes;a archava_accounting.accounts;original archava_accounting.purchase_sources;
BEGIN
 SELECT * INTO b FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 SELECT * INTO o FROM archava_accounting.purchase_origins WHERE tenant_id=NEW.tenant_id AND id=NEW.origin_id AND book_id=NEW.book_id;
 expected:=archava_accounting.purchase_effect(NEW.tenant_id,NEW.id);
 IF o.id IS NULL OR b.id IS NULL OR o.order_id IS DISTINCT FROM NEW.order_id OR expected IS NULL
 OR expected IS DISTINCT FROM jsonb_build_object('orderId',NEW.order_id,'documentId',NEW.document_id,'revision',NEW.source_revision,'effect',NEW.effect,'goodsMinor',NEW.goods_minor::text,'landedFeeMinor',NEW.landed_fee_minor::text,'actorId',NEW.actor_id,'occurredAt',NEW.occurred_at,'sourceDate',NEW.source_date,'snapshot',NEW.source_snapshot) THEN RAISE EXCEPTION 'Purchase source differs from its immutable operating effect' USING ERRCODE='23514';END IF;
 SELECT * INTO part FROM archava_accounting.purchase_effect_tax(NEW.tenant_id,NEW.id);
 unit_factor:=CASE o.policy_snapshot->>'source_unit' WHEN 'whole_idr' THEN 100 WHEN 'minor' THEN 1 END;
 IF part.tax IS NULL OR unit_factor IS NULL OR part.tax*unit_factor IS DISTINCT FROM NEW.tax_minor::numeric THEN
  RAISE EXCEPTION 'Purchase source tax differs from the tax stored on its supplier document' USING ERRCODE='23514';END IF;
 IF part.tax_code_id IS NULL THEN
  IF NEW.tax_account_id IS NOT NULL OR NEW.tax_account_revision IS NOT NULL THEN
   RAISE EXCEPTION 'Purchase source tax differs from the tax stored on its supplier document' USING ERRCODE='23514';END IF;
 ELSIF NEW.effect='bill' THEN
  IF abs(NEW.goods_minor::numeric+NEW.tax_minor)>9000000000000000 THEN
   RAISE EXCEPTION 'Purchase source minor-unit bound exceeded' USING ERRCODE='23514';END IF;
  SELECT * INTO t FROM archava_accounting.tax_codes WHERE tenant_id=NEW.tenant_id AND id=part.tax_code_id FOR SHARE;
  IF t.id IS NULL OR t.book_id IS DISTINCT FROM NEW.book_id OR t.kind<>'input' OR t.state NOT IN ('confirmed','retired') THEN
   RAISE EXCEPTION 'The supplier document tax code is not a confirmed input tax code of the posting book' USING ERRCODE='23514';END IF;
  SELECT * INTO a FROM archava_accounting.accounts WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND id=t.control_account_id FOR SHARE;
  IF a.id IS NULL OR NOT a.active THEN
   RAISE EXCEPTION 'The tax control account of the supplier document tax code is inactive' USING ERRCODE='23514';END IF;
  IF EXISTS(SELECT 1 FROM jsonb_array_elements(o.policy_snapshot->'mappings') v WHERE (v->>'accountId')::uuid=t.control_account_id) THEN
   RAISE EXCEPTION 'The tax control account is also an account of the purchase posting policy' USING ERRCODE='23514';END IF;
  IF (NEW.tax_account_id,NEW.tax_account_revision) IS DISTINCT FROM (a.id,a.revision) THEN
   RAISE EXCEPTION 'Purchase source tax differs from the tax stored on its supplier document' USING ERRCODE='23514';END IF;
 ELSE
  SELECT * INTO original FROM archava_accounting.purchase_sources WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND id=part.bill_request AND effect='bill';
  IF original.id IS NULL OR (NEW.tax_account_id,NEW.tax_account_revision) IS DISTINCT FROM (original.tax_account_id,original.tax_account_revision) THEN
   RAISE EXCEPTION 'A purchase void retains the tax control account of its supplier bill' USING ERRCODE='23514';END IF;
 END IF;
 IF EXISTS(SELECT 1 FROM archava_accounting.periods WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND state='closed' AND NEW.source_date BETWEEN start_date AND end_date) THEN RAISE EXCEPTION 'Reopen the financial source period before changing this purchase' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;

-- The invoice draft guard of 017 with the order of review: a correction or a
-- void is drafted after its invoice's issue is posted. An issue of zero has no
-- journal (a journal line cannot be zero) and holds nothing back.
CREATE OR REPLACE FUNCTION archava_accounting.guard_journal_source() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE s archava_accounting.invoice_sources;j archava_accounting.journals;ar uuid;counterpart uuid;debit uuid;credit uuid;net bigint;
BEGIN
 PERFORM 1 FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 SELECT * INTO s FROM archava_accounting.invoice_sources WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND id=NEW.source_id;
 SELECT * INTO j FROM archava_accounting.journals WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND id=NEW.journal_id;
 IF s.id IS NULL OR s.amount_minor=0 OR j.id IS NULL OR j.state<>'draft' OR j.reversal_of IS NOT NULL OR j.entry_date<>s.source_date OR j.source_ref<>'billing:'||s.id::text||':'||j.code
  OR EXISTS(SELECT 1 FROM archava_accounting.journal_sources x JOIN archava_accounting.journals y ON y.tenant_id=x.tenant_id AND y.id=x.journal_id WHERE x.tenant_id=NEW.tenant_id AND x.source_id=NEW.source_id AND y.state<>'void') THEN
  RAISE EXCEPTION 'A financial source needs one reviewed live draft' USING ERRCODE='23514';END IF;
 IF s.effect IN ('correction','void') AND EXISTS(SELECT 1 FROM archava_accounting.invoice_sources o WHERE o.tenant_id=s.tenant_id AND o.book_id=s.book_id AND o.invoice_id=s.invoice_id AND o.effect='issue' AND o.amount_minor<>0
  AND NOT EXISTS(SELECT 1 FROM archava_accounting.journal_sources x JOIN archava_accounting.journals y ON y.tenant_id=x.tenant_id AND y.id=x.journal_id WHERE x.tenant_id=o.tenant_id AND x.source_id=o.id AND y.state='posted')) THEN
  RAISE EXCEPTION 'Post the invoice issue before reviewing its correction or void' USING ERRCODE='23514';END IF;
 SELECT (v->>'accountId')::uuid INTO ar FROM jsonb_array_elements(s.policy_snapshot->'mappings') v WHERE v->>'role'='receivable';
 SELECT (v->>'accountId')::uuid INTO counterpart FROM jsonb_array_elements(s.policy_snapshot->'mappings') v WHERE v->>'role'=CASE WHEN s.effect IN ('collection','refund') THEN 'payment_clearing' ELSE 'revenue' END;
 IF s.tax_minor<>0 THEN
  net:=s.amount_minor-s.tax_minor;
  IF s.tax_account_id IS NULL OR s.effect IN ('collection','refund')
   OR (SELECT count(*) FROM archava_accounting.journal_lines WHERE tenant_id=NEW.tenant_id AND journal_id=j.id)<>(CASE WHEN net=0 THEN 2 ELSE 3 END)
   OR NOT EXISTS(SELECT 1 FROM archava_accounting.journal_lines WHERE tenant_id=NEW.tenant_id AND journal_id=j.id AND account_id=ar
    AND debit_minor=(CASE WHEN s.amount_minor>0 THEN abs(s.amount_minor) ELSE 0 END) AND credit_minor=(CASE WHEN s.amount_minor>0 THEN 0 ELSE abs(s.amount_minor) END))
   OR NOT EXISTS(SELECT 1 FROM archava_accounting.journal_lines WHERE tenant_id=NEW.tenant_id AND journal_id=j.id AND account_id=s.tax_account_id
    AND credit_minor=(CASE WHEN s.amount_minor>0 THEN abs(s.tax_minor) ELSE 0 END) AND debit_minor=(CASE WHEN s.amount_minor>0 THEN 0 ELSE abs(s.tax_minor) END))
   OR (net<>0 AND NOT EXISTS(SELECT 1 FROM archava_accounting.journal_lines WHERE tenant_id=NEW.tenant_id AND journal_id=j.id AND account_id=counterpart
    AND credit_minor=(CASE WHEN s.amount_minor>0 THEN abs(net) ELSE 0 END) AND debit_minor=(CASE WHEN s.amount_minor>0 THEN 0 ELSE abs(net) END))) THEN
   RAISE EXCEPTION 'A taxed source draft must use the captured amount, net and tax on the receivable, revenue and tax control accounts' USING ERRCODE='23514';END IF;
  RETURN NEW;
 END IF;
 debit:=CASE WHEN s.effect IN ('collection','refund') THEN counterpart ELSE ar END;
 credit:=CASE WHEN s.effect IN ('collection','refund') THEN ar ELSE counterpart END;
 IF s.amount_minor<0 THEN ar:=debit;debit:=credit;credit:=ar;END IF;
 IF (SELECT count(*) FROM archava_accounting.journal_lines WHERE tenant_id=NEW.tenant_id AND journal_id=j.id)<>2 OR NOT EXISTS(
  SELECT 1 FROM archava_accounting.journal_lines WHERE tenant_id=NEW.tenant_id AND journal_id=j.id AND account_id=debit AND debit_minor=abs(s.amount_minor) AND credit_minor=0) OR NOT EXISTS(
  SELECT 1 FROM archava_accounting.journal_lines WHERE tenant_id=NEW.tenant_id AND journal_id=j.id AND account_id=credit AND credit_minor=abs(s.amount_minor) AND debit_minor=0) THEN
  RAISE EXCEPTION 'Source draft must use the exact captured amount and account roles' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;

-- The purchase draft guard of 018 with the order of review: a void is drafted
-- after the bill it reverses is posted. The bill is the source of the prior
-- matched document version, as payable_balance() reads it.
CREATE OR REPLACE FUNCTION archava_accounting.guard_purchase_journal_source() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE s archava_accounting.purchase_sources;j archava_accounting.journals;p jsonb;expected_count integer;
BEGIN
 PERFORM 1 FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=NEW.book_id FOR UPDATE;
 SELECT * INTO s FROM archava_accounting.purchase_sources WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND id=NEW.source_id;
 SELECT policy_snapshot INTO p FROM archava_accounting.purchase_origins WHERE tenant_id=NEW.tenant_id AND id=s.origin_id AND book_id=NEW.book_id;
 SELECT * INTO j FROM archava_accounting.journals WHERE tenant_id=NEW.tenant_id AND book_id=NEW.book_id AND id=NEW.journal_id;
 SELECT count(*) INTO expected_count FROM archava_accounting.purchase_legs(NEW.tenant_id,NEW.source_id);
 IF s.id IS NULL OR expected_count=0 OR p IS NULL OR j.id IS NULL OR j.state<>'draft' OR j.reversal_of IS NOT NULL OR j.entry_date<>s.source_date OR j.source_ref<>'purchasing:'||s.id::text||':'||j.code
 OR EXISTS(SELECT 1 FROM archava_accounting.purchase_journal_sources x JOIN archava_accounting.journals y ON y.tenant_id=x.tenant_id AND y.id=x.journal_id WHERE x.tenant_id=NEW.tenant_id AND x.source_id=NEW.source_id AND y.state<>'void') THEN RAISE EXCEPTION 'A purchase source requires one reviewed live source draft' USING ERRCODE='23514';END IF;
 IF s.effect='void' AND NOT EXISTS(SELECT 1 FROM archava_accounting.purchase_journal_sources x JOIN archava_accounting.journals y ON y.tenant_id=x.tenant_id AND y.id=x.journal_id
  WHERE x.tenant_id=s.tenant_id AND x.source_id=(s.source_snapshot->'priorMatchedVersion'->>'request_id')::uuid AND y.state='posted') THEN
  RAISE EXCEPTION 'Post the supplier bill before reviewing its void' USING ERRCODE='23514';END IF;
 IF (SELECT count(*) FROM archava_accounting.journal_lines WHERE tenant_id=NEW.tenant_id AND journal_id=j.id)<>expected_count OR EXISTS(
 SELECT 1 FROM archava_accounting.purchase_legs(NEW.tenant_id,NEW.source_id) l LEFT JOIN LATERAL(SELECT CASE WHEN l.role='input_tax' THEN s.tax_account_id
  ELSE (SELECT (v->>'accountId')::uuid FROM jsonb_array_elements(p->'mappings') v WHERE v->>'role'=l.role) END AS id) account ON true
 WHERE account.id IS NULL OR NOT EXISTS(SELECT 1 FROM archava_accounting.journal_lines x WHERE x.tenant_id=NEW.tenant_id AND x.journal_id=j.id AND x.account_id=account.id AND x.debit_minor=l.debit_minor AND x.credit_minor=l.credit_minor)) THEN
  IF s.tax_minor<>0 THEN
   RAISE EXCEPTION 'A taxed purchase draft must use the captured goods, tax and total on the goods received, tax control and payable accounts' USING ERRCODE='23514';END IF;
  RAISE EXCEPTION 'Purchase draft differs from captured goods, fee or account roles' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;

-- Readiness over the tax guards outside Accounting, in the manner of
-- source_guards_complete(): each trigger exists on its table under its name,
-- runs the expected function, has the expected timing, events and
-- deferrability, no column list, no WHEN clause and no arguments, and is
-- enabled for ordinary sessions.
--  23 = BEFORE INSERT OR UPDATE FOR EACH ROW, 7 = BEFORE INSERT FOR EACH ROW,
--   5 = AFTER INSERT FOR EACH ROW (the two deferred constraint triggers).
CREATE FUNCTION archava_accounting.tax_document_guards_complete() RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog AS $$
 SELECT NOT EXISTS(SELECT 1 FROM (VALUES
 ('archava_billing','invoices','invoice_tax_guard','archava_billing.guard_invoice_tax()',23,false,false),
 ('archava_billing','invoice_corrections','correction_tax_guard','archava_billing.guard_correction_tax()',7,false,false),
 ('archava_billing','invoices','invoice_tax_identifiers_guard','archava_billing.guard_invoice_tax_identifiers()',23,false,false),
 ('archava_billing','invoices','source_invoice_complete','archava_billing.assert_order_invoice()',5,true,true),
 ('archava_billing','invoice_lines','source_invoice_lines_complete','archava_billing.assert_order_invoice()',5,true,true),
 ('archava_commerce','orders','a_tax_sales','archava_commerce.guard_order_tax()',23,false,false),
 ('archava_purchasing','supplier_documents','supplier_document_tax_guard','archava_purchasing.guard_supplier_document_tax()',23,false,false)
 ) AS required(schema_name,table_name,trigger_name,function_name,trigger_type,is_deferred,initially_deferred)
 WHERE NOT EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid=to_regclass(format('%I.%I',required.schema_name,required.table_name)) AND t.tgname=required.trigger_name
  AND t.tgfoid=to_regprocedure(required.function_name) AND t.tgtype=required.trigger_type AND t.tgenabled='O' AND NOT t.tgisinternal
  AND t.tgdeferrable=required.is_deferred AND t.tginitdeferred=required.initially_deferred AND t.tgqual IS NULL
  AND t.tgattr=''::int2vector AND t.tgnargs=0))
$$;
REVOKE ALL ON FUNCTION archava_accounting.tax_document_guards_complete() FROM PUBLIC;
