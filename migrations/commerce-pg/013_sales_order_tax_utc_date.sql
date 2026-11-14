-- Tax review (2026-11-14), Commerce part. A sales order's tax code must be a
-- confirmed output code that applies on the order date, and the order date is
-- now the UTC calendar date of the transaction clock, the calendar every tax
-- date uses until a tenant time zone exists (TAX.md). guard_order_tax() of 012
-- read current_date, the date in the session's time zone. It is replaced in
-- place with its signature, so the trigger a_tax_sales that calls it is
-- unchanged; every other rule is the same. No row is rewritten and no grant
-- changes.
CREATE OR REPLACE FUNCTION archava_commerce.guard_order_tax() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE book uuid;today date;
BEGIN
 IF TG_OP='UPDATE' THEN
  IF NEW.tax_code IS DISTINCT FROM OLD.tax_code OR NEW.price_includes_tax IS DISTINCT FROM OLD.price_includes_tax THEN
   RAISE EXCEPTION 'Sales order tax code and price basis are immutable' USING ERRCODE='23514';END IF;
  RETURN NEW;
 END IF;
 IF NEW.tax_code IS NULL AND NEW.price_includes_tax IS NULL THEN RETURN NEW;END IF;
 IF NEW.tax_code IS NULL OR NEW.price_includes_tax IS NULL THEN
  RAISE EXCEPTION 'A sales order tax code and its price basis are stated together' USING ERRCODE='23514';END IF;
 IF NEW.company_id IS NULL THEN
  RAISE EXCEPTION 'A sales order tax code needs a company' USING ERRCODE='23514';END IF;
 IF to_regclass('archava_accounting.tax_codes') IS NOT NULL THEN
  SELECT id INTO book FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND company_id=NEW.company_id;
 END IF;
 IF book IS NULL THEN
  RAISE EXCEPTION 'Sales order tax code is not a tax code of the company book' USING ERRCODE='23514';END IF;
 today:=(transaction_timestamp() AT TIME ZONE 'UTC')::date;
 -- A withdrawn version never applied, so it does not make a code known.
 IF NOT EXISTS(SELECT 1 FROM archava_accounting.tax_codes t WHERE t.tenant_id=NEW.tenant_id AND t.book_id=book AND t.code=NEW.tax_code AND t.state<>'withdrawn') THEN
  RAISE EXCEPTION 'Sales order tax code is not a tax code of the company book' USING ERRCODE='23514';END IF;
 IF EXISTS(SELECT 1 FROM archava_accounting.tax_codes t WHERE t.tenant_id=NEW.tenant_id AND t.book_id=book AND t.code=NEW.tax_code AND t.state<>'withdrawn' AND t.kind<>'output') THEN
  RAISE EXCEPTION 'A sales order needs an output tax code' USING ERRCODE='23514';END IF;
 IF EXISTS(SELECT 1 FROM archava_accounting.tax_codes t WHERE t.tenant_id=NEW.tenant_id AND t.book_id=book AND t.code=NEW.tax_code AND t.kind='output'
  AND t.state IN ('confirmed','retired') AND today BETWEEN t.effective_from AND coalesce(t.effective_to,DATE 'infinity')) THEN
  RETURN NEW;END IF;
 IF EXISTS(SELECT 1 FROM archava_accounting.tax_codes t WHERE t.tenant_id=NEW.tenant_id AND t.book_id=book AND t.code=NEW.tax_code AND t.state='entered' AND t.effective_from<=today) THEN
  RAISE EXCEPTION 'The sales order tax code is entered but not confirmed' USING ERRCODE='23514';END IF;
 RAISE EXCEPTION 'No confirmed version of the sales order tax code applies on the order date' USING ERRCODE='23514';
END $$;
