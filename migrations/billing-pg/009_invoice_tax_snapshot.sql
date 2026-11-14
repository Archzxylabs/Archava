-- Tax stage B: an invoice made from a company sales order that names a tax code
-- keeps what it was taxed with. gross_minor stays the amount the customer owes
-- and stays immutable; tax_minor is the tax part of it (net = gross - tax) and
-- is immutable with the snapshot: the tax code version, its kind, rate and base
-- fraction and the price basis. correction_tax_minor is the tax part of
-- correction_minor, and every correction row stores its own tax part.
-- Existing rows are untaxed (tax 0, no snapshot) and are not rewritten; manual
-- invoices cannot carry tax. Nothing here posts tax to a ledger.
ALTER TABLE archava_billing.invoices
 ADD COLUMN tax_minor bigint NOT NULL DEFAULT 0,ADD COLUMN correction_tax_minor bigint NOT NULL DEFAULT 0,
 ADD COLUMN tax_code_id uuid,ADD COLUMN tax_code text,ADD COLUMN tax_code_version integer,ADD COLUMN tax_kind text,
 ADD COLUMN tax_rate_numerator integer,ADD COLUMN tax_rate_denominator integer,
 ADD COLUMN tax_base_numerator integer,ADD COLUMN tax_base_denominator integer,
 ADD COLUMN price_includes_tax boolean,
 ADD CONSTRAINT invoice_tax_snapshot CHECK((
  (tax_code_id IS NULL AND tax_code IS NULL AND tax_code_version IS NULL AND tax_kind IS NULL
   AND tax_rate_numerator IS NULL AND tax_rate_denominator IS NULL AND tax_base_numerator IS NULL AND tax_base_denominator IS NULL
   AND price_includes_tax IS NULL AND tax_minor=0 AND correction_tax_minor=0)
  OR (tax_code_id IS NOT NULL AND tax_code IS NOT NULL AND tax_code_version>0 AND tax_kind='output'
   AND tax_rate_denominator BETWEEN 1 AND 1000000 AND tax_rate_numerator BETWEEN 0 AND tax_rate_denominator
   AND tax_base_denominator BETWEEN 1 AND 1000000 AND tax_base_numerator BETWEEN 1 AND tax_base_denominator
   AND price_includes_tax IS NOT NULL AND tax_minor BETWEEN 0 AND gross_minor)) IS TRUE);
ALTER TABLE archava_billing.invoice_corrections ADD COLUMN tax_minor bigint NOT NULL DEFAULT 0;

-- Integer tax arithmetic (TAX.md rules 4 and 5), the same as the domain's:
-- a quotient rounded half away from zero, the tax a net amount carries and the
-- tax a gross amount contains. numeric keeps every intermediate exact.
CREATE FUNCTION archava_billing.tax_round(n numeric,d numeric) RETURNS bigint LANGUAGE sql IMMUTABLE SET search_path=pg_catalog AS $$
 SELECT (sign(n)*div(2*abs(n)+d,2*d))::bigint
$$;
CREATE FUNCTION archava_billing.tax_on_net(net bigint,rate_n integer,rate_d integer,base_n integer,base_d integer) RETURNS bigint LANGUAGE sql IMMUTABLE SET search_path=pg_catalog AS $$
 SELECT archava_billing.tax_round(net::numeric*(base_n::numeric*rate_n),base_d::numeric*rate_d)
$$;
CREATE FUNCTION archava_billing.tax_in_gross(gross bigint,rate_n integer,rate_d integer,base_n integer,base_d integer) RETURNS bigint LANGUAGE sql IMMUTABLE SET search_path=pg_catalog AS $$
 SELECT gross-archava_billing.tax_round(gross::numeric*(base_d::numeric*rate_d),base_d::numeric*rate_d+base_n::numeric*rate_n)
$$;

-- Billing migrates before Accounting, so the posting policy is read late-bound
-- (as Commerce 011 reads the book): an install without it has no policy.
CREATE FUNCTION archava_billing.invoice_posting_policy(t text,c uuid) RETURNS boolean LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF c IS NULL OR to_regclass('archava_accounting.invoice_policies') IS NULL THEN RETURN false;END IF;
 RETURN EXISTS(SELECT 1 FROM archava_accounting.invoice_policies p JOIN archava_accounting.books b ON b.tenant_id=p.tenant_id AND b.id=p.book_id
  WHERE p.tenant_id=t AND b.company_id=c);
END $$;

-- The snapshot is fixed at insert and names a confirmed output version of the
-- company's book that applies on the invoice date: the database's date of
-- created_at, the only date an invoice has. The tax part of the corrections is
-- never entered: after every change it is the tax contained in the corrected
-- total (gross + corrections) less the original tax, so a correction that
-- brings the total to zero brings net and tax to zero as well.
-- Until tax posts to the ledger (stage C) a taxed invoice is refused for a
-- company whose book has an invoice posting policy, at creation and again at
-- issue, so the invoice bridge never captures it as if it were untaxed.
CREATE FUNCTION archava_billing.guard_invoice_tax() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE taxed boolean;
BEGIN
 taxed:=NEW.tax_code_id IS NOT NULL;
 IF TG_OP='UPDATE' THEN
  IF (NEW.tax_minor,NEW.tax_code_id,NEW.tax_code,NEW.tax_code_version,NEW.tax_kind,NEW.tax_rate_numerator,NEW.tax_rate_denominator,NEW.tax_base_numerator,NEW.tax_base_denominator,NEW.price_includes_tax)
   IS DISTINCT FROM (OLD.tax_minor,OLD.tax_code_id,OLD.tax_code,OLD.tax_code_version,OLD.tax_kind,OLD.tax_rate_numerator,OLD.tax_rate_denominator,OLD.tax_base_numerator,OLD.tax_base_denominator,OLD.price_includes_tax) THEN
   RAISE EXCEPTION 'Invoice tax snapshot is immutable' USING ERRCODE='23514';END IF;
  IF NEW.correction_tax_minor IS DISTINCT FROM (CASE WHEN taxed THEN
    archava_billing.tax_in_gross(NEW.gross_minor+NEW.correction_minor,NEW.tax_rate_numerator,NEW.tax_rate_denominator,NEW.tax_base_numerator,NEW.tax_base_denominator)-NEW.tax_minor
   ELSE 0 END) THEN
   RAISE EXCEPTION 'Invoice correction tax must follow the stored tax rule' USING ERRCODE='23514';END IF;
  IF taxed AND OLD.status='draft' AND NEW.status='issued' AND archava_billing.invoice_posting_policy(NEW.tenant_id,NEW.company_id) THEN
   RAISE EXCEPTION 'Tax posting is not available yet for a book with invoice posting' USING ERRCODE='23514';END IF;
  RETURN NEW;
 END IF;
 IF NEW.correction_tax_minor IS DISTINCT FROM 0 THEN
  RAISE EXCEPTION 'Invoice correction tax must follow the stored tax rule' USING ERRCODE='23514';END IF;
 IF NOT taxed THEN RETURN NEW;END IF;
 IF NEW.source_order_id IS NULL OR NEW.company_id IS NULL THEN
  RAISE EXCEPTION 'Only an invoice made from a company sales order can carry tax' USING ERRCODE='23514';END IF;
 IF to_regclass('archava_accounting.tax_codes') IS NULL THEN
  RAISE EXCEPTION 'No confirmed version of the invoice tax code applies on the invoice date' USING ERRCODE='23514';END IF;
 IF NOT EXISTS(SELECT 1 FROM archava_accounting.tax_codes t JOIN archava_accounting.books b ON b.tenant_id=t.tenant_id AND b.id=t.book_id
  WHERE t.tenant_id=NEW.tenant_id AND t.id=NEW.tax_code_id AND b.company_id=NEW.company_id
  AND t.code=NEW.tax_code AND t.version=NEW.tax_code_version AND t.kind=NEW.tax_kind
  AND t.rate_numerator=NEW.tax_rate_numerator AND t.rate_denominator=NEW.tax_rate_denominator
  AND t.base_numerator=NEW.tax_base_numerator AND t.base_denominator=NEW.tax_base_denominator
  AND t.state IN ('confirmed','retired') AND NEW.created_at::date BETWEEN t.effective_from AND coalesce(t.effective_to,DATE 'infinity')) THEN
  RAISE EXCEPTION 'No confirmed version of the invoice tax code applies on the invoice date' USING ERRCODE='23514';END IF;
 IF archava_billing.invoice_posting_policy(NEW.tenant_id,NEW.company_id) THEN
  RAISE EXCEPTION 'Tax posting is not available yet for a book with invoice posting' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER invoice_tax_guard BEFORE INSERT OR UPDATE ON archava_billing.invoices FOR EACH ROW EXECUTE FUNCTION archava_billing.guard_invoice_tax();

-- A correction row carries exactly the change it made to its invoice's
-- correction tax: the invoice is corrected first, then the row is appended.
CREATE FUNCTION archava_billing.guard_correction_tax() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE held bigint;recorded bigint;
BEGIN
 SELECT correction_tax_minor INTO held FROM archava_billing.invoices WHERE tenant_id=NEW.tenant_id AND id=NEW.invoice_id;
 SELECT coalesce(sum(tax_minor),0) INTO recorded FROM archava_billing.invoice_corrections WHERE tenant_id=NEW.tenant_id AND invoice_id=NEW.invoice_id;
 IF NEW.tax_minor IS DISTINCT FROM coalesce(held,0)-recorded THEN
  RAISE EXCEPTION 'Correction tax must equal the change in its invoice tax' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER correction_tax_guard BEFORE INSERT ON archava_billing.invoice_corrections FOR EACH ROW EXECUTE FUNCTION archava_billing.guard_correction_tax();

-- The provenance guard of 005, replaced in place (same signature, so the two
-- deferred triggers that call it are unchanged). The source order's total is
-- its priced lines: the invoice's net with an exclusive basis (gross - tax) and
-- its gross with an inclusive one or without tax. The invoice carries the
-- order's tax code and basis, and its tax is recomputed here from the order
-- total and the snapshot's fractions. Lines still match the order's lines; tax
-- is a document amount, not a line. The order columns are read through jsonb
-- so an install whose Commerce ledger predates them reads "no tax code".
CREATE OR REPLACE FUNCTION archava_billing.assert_order_invoice() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE invoice archava_billing.invoices; source archava_commerce.orders;
  wanted_invoice uuid; expected_lines integer; actual_lines integer;
  source_tax text; source_inclusive boolean; expected_tax bigint;
BEGIN
  IF TG_TABLE_NAME='invoices' THEN wanted_invoice:=NEW.id;
  ELSE wanted_invoice:=NEW.invoice_id;
  END IF;
  SELECT * INTO STRICT invoice FROM archava_billing.invoices
    WHERE tenant_id=NEW.tenant_id AND id=wanted_invoice;
  IF invoice.source_order_id IS NULL THEN
    IF EXISTS(SELECT 1 FROM archava_billing.invoice_lines
      WHERE tenant_id=invoice.tenant_id AND invoice_id=invoice.id AND source_order_line_id IS NOT NULL) THEN
      RAISE EXCEPTION 'Manual invoice cannot claim source lines';
    END IF;
    RETURN NULL;
  END IF;
  SELECT * INTO STRICT source FROM archava_commerce.orders
    WHERE tenant_id=invoice.tenant_id AND id=invoice.source_order_id FOR UPDATE;
  source_tax:=to_jsonb(source)->>'tax_code';
  source_inclusive:=(to_jsonb(source)->>'price_includes_tax')::boolean;
  expected_tax:=CASE WHEN source_tax IS NULL OR invoice.tax_code_id IS NULL THEN 0
    WHEN source_inclusive THEN archava_billing.tax_in_gross(source.total_estimate,invoice.tax_rate_numerator,invoice.tax_rate_denominator,invoice.tax_base_numerator,invoice.tax_base_denominator)
    ELSE archava_billing.tax_on_net(source.total_estimate,invoice.tax_rate_numerator,invoice.tax_rate_denominator,invoice.tax_base_numerator,invoice.tax_base_denominator) END;
  IF source_tax IS DISTINCT FROM invoice.tax_code OR source_inclusive IS DISTINCT FROM invoice.price_includes_tax OR
     invoice.tax_minor IS DISTINCT FROM expected_tax THEN
    RAISE EXCEPTION 'Invoice tax must match the source order tax code and its arithmetic' USING ERRCODE='23514';
  END IF;
  IF source.managed_by IS NOT NULL OR source.state<>'fulfilled' OR
     source.revision<>invoice.source_order_revision OR
     source.customer_name<>invoice.customer_ref OR source.currency<>invoice.currency OR
     source.total_estimate<>invoice.gross_minor-(CASE WHEN source_inclusive IS FALSE THEN invoice.tax_minor ELSE 0 END) OR
     invoice.consumer_ref<>'commerce:'||source.id::text THEN
    RAISE EXCEPTION 'Invoice must match the fulfilled source order snapshot';
  END IF;
  SELECT count(*) INTO expected_lines FROM archava_commerce.order_lines
    WHERE tenant_id=source.tenant_id AND order_id=source.id;
  SELECT count(*) INTO actual_lines FROM archava_billing.invoice_lines
    WHERE tenant_id=invoice.tenant_id AND invoice_id=invoice.id;
  IF expected_lines=0 OR expected_lines<>actual_lines OR EXISTS(
    SELECT 1 FROM archava_billing.invoice_lines line
    LEFT JOIN archava_commerce.order_lines original
      ON original.tenant_id=line.tenant_id AND original.id=line.source_order_line_id
    WHERE line.tenant_id=invoice.tenant_id AND line.invoice_id=invoice.id AND
      (original.id IS NULL OR original.order_id<>source.id OR original.returned_quantity<>0 OR
       original.title<>line.description OR original.quantity<>line.quantity OR
       original.price::bigint<>line.unit_minor OR
       original.quantity::bigint*original.price<>line.line_total_minor)) THEN
    RAISE EXCEPTION 'Invoice lines must exactly match the unreturned source order';
  END IF;
  RETURN NULL;
END
$$;
