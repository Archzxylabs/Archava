-- Tax stage D: a taxed supplier bill posts its input tax to the ledger. A
-- captured purchase source keeps the tax of its supplier document version
-- (ledger minor units, the same unit factor as the goods) and the tax control
-- account of the tax code version stored on that document version, with that
-- account's revision at capture. goods_minor stays the net of goods; the
-- payable of a bill is goods_minor + tax_minor. The bill source takes the
-- account from the tax code; the void of a bill keeps the account of its bill
-- source. A receipt carries no tax. The purchase posting policy and its four
-- roles are unchanged. A source of an untaxed document is stored and drafted
-- exactly as before.
-- Earlier SQL bytes are unchanged: the five functions are replaced in place
-- with their signatures, so their OIDs, owners, grants and the triggers
-- purchase_guards_complete() and payable_guards_complete() identify by OID are
-- kept. Existing rows get tax 0 and no tax account; nothing is imported and no
-- row is rewritten.
DO $$ BEGIN
 PERFORM pg_advisory_xact_lock(hashtext('archava-serving-writer-takeover'));
 IF to_regclass('archava_control.writer_lease') IS NOT NULL THEN
  PERFORM 1 FROM archava_control.writer_lease WHERE lease_name='archava-serving' FOR UPDATE;
 END IF;
END $$;
ALTER TABLE archava_accounting.purchase_sources
 ADD COLUMN tax_minor bigint NOT NULL DEFAULT 0,ADD COLUMN tax_account_id uuid,ADD COLUMN tax_account_revision integer,
 ADD CONSTRAINT purchase_source_tax CHECK((
  (tax_account_id IS NULL AND tax_account_revision IS NULL AND tax_minor=0)
  OR (tax_account_id IS NOT NULL AND tax_account_revision>0
   AND ((effect='bill' AND tax_minor BETWEEN 0 AND goods_minor) OR (effect='void' AND tax_minor BETWEEN goods_minor AND 0))
   AND goods_minor::numeric+tax_minor BETWEEN -9000000000000000 AND 9000000000000000)) IS TRUE),
 ADD CONSTRAINT purchase_source_tax_account FOREIGN KEY(tenant_id,book_id,tax_account_id) REFERENCES archava_accounting.accounts(tenant_id,book_id,id);

-- purchase_effect keeps its result (014): the goods, the fee and the evidence
-- of the effect. The tax part of the same effect is a second function, so the
-- effect an untaxed source is compared with does not change. It reads what
-- Purchasing stored on the document version (Purchasing 008): the entered tax
-- and the tax code of the matched version for a bill, the negated tax and the
-- tax code of the prior matched version for a void, with the request of that
-- version (the bill source), and zero without a code for anything else. A
-- version stored before Purchasing 008 has no tax. The amount is in the
-- document's source unit; the caller applies the policy's unit factor.
CREATE FUNCTION archava_accounting.purchase_effect_tax(wanted_tenant text,wanted_request uuid) RETURNS TABLE(tax numeric,tax_code_id uuid,bill_request uuid)
LANGUAGE plpgsql VOLATILE SET search_path=pg_catalog AS $$
DECLARE a archava_purchasing.audit;v archava_purchasing.supplier_document_versions;prior archava_purchasing.supplier_document_versions;
BEGIN
 tax:=0;
 SELECT * INTO a FROM archava_purchasing.audit WHERE tenant_id=wanted_tenant AND request_id=wanted_request;
 IF a.operation IN ('document.match','document.void') THEN
  SELECT * INTO v FROM archava_purchasing.supplier_document_versions WHERE tenant_id=wanted_tenant AND request_id=wanted_request AND document_id=a.record_id AND revision=a.revision;
  IF a.operation='document.match' AND v.snapshot->>'state'='matched' THEN
   tax:=coalesce((v.snapshot->>'tax_minor')::numeric,0);tax_code_id:=(v.snapshot->>'tax_code_id')::uuid;
  ELSIF a.operation='document.void' AND v.snapshot->>'state'='void' THEN
   SELECT * INTO prior FROM archava_purchasing.supplier_document_versions WHERE tenant_id=wanted_tenant AND document_id=v.document_id AND revision=v.revision-1;
   IF prior.snapshot->>'state'='matched' THEN
    tax:=-coalesce((prior.snapshot->>'tax_minor')::numeric,0);tax_code_id:=(prior.snapshot->>'tax_code_id')::uuid;bill_request:=prior.request_id;
   END IF;
  END IF;
 END IF;
 RETURN NEXT;
END $$;

-- The source guard of 004 with the tax checks added. The captured tax is the
-- document version's entered tax for the effect times the unit factor of the
-- policy the order's origin pinned. A source of a taxed document names the tax
-- control account: for a bill the account of the document's tax code version,
-- which must still be a confirmed or retired input code of this book whose
-- account is active and is none of the policy's accounts (an account's
-- identity and class never change, so its revision is not compared:
-- reactivating a deactivated account releases the bill), with the account's
-- current revision; for a void the account of the bill source it reverses.
-- Applicability on a date is not evaluated again: the document version's
-- stored snapshot governs.
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
  IF EXISTS(SELECT 1 FROM jsonb_array_elements(o.policy_snapshot->'mappings') v WHERE v->>'accountId'=t.control_account_id::text) THEN
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

-- The capture of 014 with the tax part and the tax control account added. It
-- only derives the values; every refusal is the guard's, so a direct insert is
-- held to the same rules. Still quiet without a company, a book or a policy,
-- and for an order confirmed before its policy.
CREATE OR REPLACE FUNCTION archava_accounting.capture_purchase_source() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE po uuid;company uuid;order_currency text;b archava_accounting.books;p archava_accounting.purchase_policies;o archava_accounting.purchase_origins;a archava_purchasing.audit;e jsonb;held jsonb;
 part record;unit_factor numeric;tax_account uuid;tax_revision integer;
BEGIN
 IF OLD.receipt IS NOT NULL OR NEW.receipt IS NULL THEN RETURN NEW;END IF;
 SELECT * INTO a FROM archava_purchasing.audit WHERE tenant_id=NEW.tenant_id AND request_id=NEW.request_id;
 IF a.operation='order.confirm' THEN
  SELECT company_id,currency INTO company,order_currency FROM archava_purchasing.orders WHERE tenant_id=NEW.tenant_id AND id=a.record_id;
  IF company IS NULL THEN RETURN NEW;END IF;
  SELECT * INTO b FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND company_id=company FOR UPDATE;
  IF b.id IS NULL THEN RETURN NEW;END IF;
  SELECT * INTO p FROM archava_accounting.purchase_policies WHERE tenant_id=NEW.tenant_id AND book_id=b.id;
  SELECT snapshot->'record' INTO held FROM archava_accounting.audit WHERE tenant_id=NEW.tenant_id AND resource_kind='purchase_policy' AND resource_id=p.id AND revision=p.revision;
  IF p.id IS NOT NULL THEN
   IF order_currency IS DISTINCT FROM b.currency THEN RAISE EXCEPTION 'Purchase order currency differs from its company book' USING ERRCODE='23514';END IF;
   INSERT INTO archava_accounting.purchase_origins(tenant_id,id,book_id,policy_id,policy_revision,policy_snapshot,order_id,order_revision,actor_id,occurred_at) VALUES(NEW.tenant_id,NEW.request_id,b.id,p.id,p.revision,held,a.record_id,a.revision,a.actor_id,a.occurred_at);
  END IF;
  RETURN NEW;
 ELSIF a.operation='order.receive' THEN po:=a.record_id;
 ELSIF a.operation IN ('document.match','document.void') THEN SELECT order_id INTO po FROM archava_purchasing.supplier_documents WHERE tenant_id=NEW.tenant_id AND id=a.record_id;
 ELSE RETURN NEW;
 END IF;
 SELECT * INTO o FROM archava_accounting.purchase_origins WHERE tenant_id=NEW.tenant_id AND order_id=po;
 -- No implicit import of orders confirmed before explicit purchase policy setup.
 IF o.id IS NULL THEN RETURN NEW;END IF;
 PERFORM 1 FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND id=o.book_id FOR UPDATE;
 e:=archava_accounting.purchase_effect(NEW.tenant_id,NEW.request_id);
 IF e IS NULL THEN RETURN NEW;END IF;
 SELECT * INTO part FROM archava_accounting.purchase_effect_tax(NEW.tenant_id,NEW.request_id);
 unit_factor:=CASE o.policy_snapshot->>'source_unit' WHEN 'whole_idr' THEN 100 WHEN 'minor' THEN 1 END;
 IF part.tax_code_id IS NOT NULL THEN
  IF e->>'effect'='bill' THEN
   SELECT t.control_account_id,c.revision INTO tax_account,tax_revision FROM archava_accounting.tax_codes t
    JOIN archava_accounting.accounts c ON c.tenant_id=t.tenant_id AND c.book_id=t.book_id AND c.id=t.control_account_id
    WHERE t.tenant_id=NEW.tenant_id AND t.book_id=o.book_id AND t.id=part.tax_code_id;
  ELSE
   SELECT s.tax_account_id,s.tax_account_revision INTO tax_account,tax_revision FROM archava_accounting.purchase_sources s
    WHERE s.tenant_id=NEW.tenant_id AND s.book_id=o.book_id AND s.id=part.bill_request AND s.effect='bill';
  END IF;
 END IF;
 INSERT INTO archava_accounting.purchase_sources(tenant_id,id,book_id,origin_id,order_id,document_id,source_revision,effect,goods_minor,landed_fee_minor,source_snapshot,actor_id,occurred_at,source_date,tax_minor,tax_account_id,tax_account_revision)
 VALUES(NEW.tenant_id,NEW.request_id,o.book_id,o.id,po,(e->>'documentId')::uuid,(e->>'revision')::integer,e->>'effect',(e->>'goodsMinor')::bigint,(e->>'landedFeeMinor')::bigint,e->'snapshot',e->>'actorId',(e->>'occurredAt')::timestamptz,(e->>'sourceDate')::date,
  part.tax*unit_factor,tax_account,tax_revision);
 RETURN NEW;
END $$;

-- The legs of 004 with the tax of a bill and of its void. The payable is the
-- goods plus the tax; the tax leg has the role `input_tax`, which is not a
-- role of the purchase policy: its account is the source's tax control
-- account. A journal line cannot be zero, so a source whose tax is zero has
-- the two legs it had before.
CREATE OR REPLACE FUNCTION archava_accounting.purchase_legs(wanted_tenant text,wanted_source uuid) RETURNS TABLE(role text,debit_minor bigint,credit_minor bigint) LANGUAGE sql STABLE SET search_path=pg_catalog AS $$
 WITH s AS(SELECT * FROM archava_accounting.purchase_sources WHERE tenant_id=wanted_tenant AND id=wanted_source), legs AS(
 SELECT 'inventory'::text AS role,goods_minor+landed_fee_minor AS debit_minor,0::bigint AS credit_minor FROM s WHERE effect='receipt'
 UNION ALL SELECT 'goods_received_not_invoiced',0,goods_minor FROM s WHERE effect='receipt'
 UNION ALL SELECT 'landed_fee_accrual',0,landed_fee_minor FROM s WHERE effect='receipt'
 UNION ALL SELECT 'goods_received_not_invoiced',goods_minor,0 FROM s WHERE effect='bill'
 UNION ALL SELECT 'input_tax',tax_minor,0 FROM s WHERE effect='bill'
 UNION ALL SELECT 'payable',0,goods_minor+tax_minor FROM s WHERE effect='bill'
 UNION ALL SELECT 'payable',-(goods_minor+tax_minor),0 FROM s WHERE effect='void'
 UNION ALL SELECT 'goods_received_not_invoiced',0,-goods_minor FROM s WHERE effect='void'
 UNION ALL SELECT 'input_tax',0,-tax_minor FROM s WHERE effect='void'
 )SELECT * FROM legs WHERE debit_minor<>0 OR credit_minor<>0
$$;

-- The draft guard of 004. Every leg posts to the account its role has in the
-- origin's policy snapshot, except the tax leg, which posts to the tax control
-- account captured with the source. A source with tax is refused with its own
-- sentence.
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
 IF (SELECT count(*) FROM archava_accounting.journal_lines WHERE tenant_id=NEW.tenant_id AND journal_id=j.id)<>expected_count OR EXISTS(
 SELECT 1 FROM archava_accounting.purchase_legs(NEW.tenant_id,NEW.source_id) l LEFT JOIN LATERAL(SELECT CASE WHEN l.role='input_tax' THEN s.tax_account_id
  ELSE (SELECT (v->>'accountId')::uuid FROM jsonb_array_elements(p->'mappings') v WHERE v->>'role'=l.role) END AS id) account ON true
 WHERE account.id IS NULL OR NOT EXISTS(SELECT 1 FROM archava_accounting.journal_lines x WHERE x.tenant_id=NEW.tenant_id AND x.journal_id=j.id AND x.account_id=account.id AND x.debit_minor=l.debit_minor AND x.credit_minor=l.credit_minor)) THEN
  IF s.tax_minor<>0 THEN
   RAISE EXCEPTION 'A taxed purchase draft must use the captured goods, tax and total on the goods received, tax control and payable accounts' USING ERRCODE='23514';END IF;
  RAISE EXCEPTION 'Purchase draft differs from captured goods, fee or account roles' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;

-- The payable balance of 009 in gross: what is owed for a bill is its goods
-- plus its tax, and a posted void takes both away. Payments, reservations and
-- the result keys are unchanged; a bill without tax has the balance it had.
CREATE OR REPLACE FUNCTION archava_accounting.payable_balance(wanted_tenant text,wanted_source uuid,excluded uuid DEFAULT NULL)
 RETURNS jsonb LANGUAGE sql VOLATILE SET search_path=pg_catalog AS $$
 WITH source AS(SELECT s.* FROM archava_accounting.purchase_sources s WHERE s.tenant_id=wanted_tenant AND s.id=wanted_source AND s.effect='bill'),
 recognized AS(SELECT s.*,s.goods_minor::numeric+s.tax_minor AS gross,
 EXISTS(SELECT 1 FROM archava_accounting.purchase_journal_sources x JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id WHERE x.tenant_id=s.tenant_id AND x.source_id=s.id AND j.state='posted') posted,
 coalesce((SELECT sum(v.goods_minor::numeric+v.tax_minor) FROM archava_accounting.purchase_sources v WHERE v.tenant_id=s.tenant_id AND v.book_id=s.book_id AND v.effect='void' AND v.source_snapshot->'priorMatchedVersion'->>'request_id'=s.id::text AND EXISTS(SELECT 1 FROM archava_accounting.purchase_journal_sources x JOIN archava_accounting.journals j ON j.tenant_id=x.tenant_id AND j.id=x.journal_id WHERE x.tenant_id=v.tenant_id AND x.source_id=v.id AND j.state='posted')),0) correction
 FROM source s),
 used AS(SELECT coalesce(sum(CASE WHEN p.effect='payment' THEN p.amount_minor::numeric ELSE -p.amount_minor::numeric END) FILTER(WHERE j.state='posted'),0) paid,
 coalesce(sum(p.amount_minor::numeric) FILTER(WHERE p.effect='payment' AND j.state='draft'),0) reserved
 FROM archava_accounting.payable_settlements p JOIN archava_accounting.journals j ON j.tenant_id=p.tenant_id AND j.id=p.id
 WHERE p.tenant_id=wanted_tenant AND p.source_id=wanted_source AND (excluded IS NULL OR p.id<>excluded))
 SELECT jsonb_build_object('originalMinor',s.gross::text,'recognizedMinor',(CASE WHEN posted THEN s.gross+correction ELSE 0 END)::text,
 'postedPaymentMinor',u.paid::text,'reservedPaymentMinor',u.reserved::text,
 'outstandingMinor',(CASE WHEN posted THEN s.gross+correction ELSE 0 END-u.paid)::text,
 'availableMinor',(CASE WHEN posted THEN s.gross+correction ELSE 0 END-u.paid-u.reserved)::text,'billPosted',posted)
 FROM recognized s CROSS JOIN used u
$$;
