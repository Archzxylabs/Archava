-- Tax review (2026-11-14), Billing part. No table, column or grant changes and
-- no row is rewritten.
--
-- 1. The invoice date is the UTC calendar date of the invoice's created_at.
--    guard_invoice_tax() of 010 is replaced in place with its signature, so
--    the trigger invoice_tax_guard that calls it is unchanged. It read
--    created_at::date, the date in the session's time zone; it now reads
--    (created_at AT TIME ZONE 'UTC')::date, the calendar the accounting
--    bridge dates sources by, until a tenant time zone exists.
-- 2. A taxed invoice is created at the clock of its own transaction. The
--    billing command stamps created_at with one clock_timestamp() sample taken
--    inside its transaction, rendered to the millisecond, so a true value lies
--    between the transaction's start (less the truncated millisecond) and the
--    clock at the insert. One second of tolerance on each side covers the
--    truncation and a small clock adjustment and nothing else: a direct insert
--    can no longer state an earlier created_at to reach a retired tax code
--    version, or a later one to reach a version that does not apply yet.
--    created_at is already immutable after the insert (001). An untaxed
--    invoice is not held to this.
-- 3. assert_order_invoice() (005, replaced by 009), guard_invoice_source()
--    (005) and guard_customer_contact() (006) were created without a pinned
--    search_path. Every relation, type and function their bodies name is
--    schema-qualified or lives in pg_catalog (to_jsonb, count), so the setting
--    is pinned with ALTER FUNCTION: bodies, OIDs, owners and the triggers that
--    call them are unchanged. A later CREATE OR REPLACE of one of them must
--    carry SET search_path=pg_catalog itself.
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
 IF (NEW.created_at BETWEEN transaction_timestamp()-interval '1 second' AND clock_timestamp()+interval '1 second') IS NOT TRUE THEN
  RAISE EXCEPTION 'A taxed invoice is created at the time of its own transaction' USING ERRCODE='23514';END IF;
 IF to_regclass('archava_accounting.tax_codes') IS NULL THEN
  RAISE EXCEPTION 'No confirmed version of the invoice tax code applies on the invoice date' USING ERRCODE='23514';END IF;
 IF NOT EXISTS(SELECT 1 FROM archava_accounting.tax_codes t JOIN archava_accounting.books b ON b.tenant_id=t.tenant_id AND b.id=t.book_id
  WHERE t.tenant_id=NEW.tenant_id AND t.id=NEW.tax_code_id AND b.company_id=NEW.company_id
  AND t.code=NEW.tax_code AND t.version=NEW.tax_code_version AND t.kind=NEW.tax_kind
  AND t.rate_numerator=NEW.tax_rate_numerator AND t.rate_denominator=NEW.tax_rate_denominator
  AND t.base_numerator=NEW.tax_base_numerator AND t.base_denominator=NEW.tax_base_denominator
  AND t.state IN ('confirmed','retired') AND (NEW.created_at AT TIME ZONE 'UTC')::date BETWEEN t.effective_from AND coalesce(t.effective_to,DATE 'infinity')) THEN
  RAISE EXCEPTION 'No confirmed version of the invoice tax code applies on the invoice date' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
ALTER FUNCTION archava_billing.assert_order_invoice() SET search_path=pg_catalog;
ALTER FUNCTION archava_billing.guard_invoice_source() SET search_path=pg_catalog;
ALTER FUNCTION archava_billing.guard_customer_contact() SET search_path=pg_catalog;
