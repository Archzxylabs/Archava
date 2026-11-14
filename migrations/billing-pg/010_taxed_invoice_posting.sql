-- Tax stage C: the invoice bridge now captures a taxed invoice with its tax
-- (Accounting 017), so the temporary stop of 009 is lifted. guard_invoice_tax()
-- is replaced in place with its signature, so the trigger that calls it is
-- unchanged; it no longer refuses a taxed invoice, at creation or at issue,
-- because the company's book has an invoice posting policy. Everything else it
-- enforces is the same: the snapshot is immutable, the correction tax follows
-- the stored rule, only an invoice made from a company sales order carries tax,
-- and the snapshot names a confirmed output version of the company's book that
-- applies on the invoice date. invoice_posting_policy() stays installed and is
-- no longer called. No row is rewritten.
CREATE OR REPLACE FUNCTION archava_billing.guard_invoice_tax() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
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
 RETURN NEW;
END $$;
