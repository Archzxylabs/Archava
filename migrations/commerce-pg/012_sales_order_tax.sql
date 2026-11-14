-- Tax stage B: a company sales order may name one output tax code of its
-- company's book and state whether its prices include the tax. The order
-- stores the code text and the price basis only; it snapshots no rate and its
-- lines and total_estimate stay the priced integers (the net with an exclusive
-- basis, the gross with an inclusive one). The invoice made from the order
-- takes the rate (Billing 009). Existing rows are not rewritten and an order
-- without a tax code behaves as before.
ALTER TABLE archava_commerce.orders
 ADD COLUMN tax_code text,ADD COLUMN price_includes_tax boolean,
 ADD CONSTRAINT sales_tax_pair CHECK((tax_code IS NULL)=(price_includes_tax IS NULL)),
 ADD CONSTRAINT sales_tax_company CHECK(tax_code IS NULL OR company_id IS NOT NULL),
 ADD CONSTRAINT sales_tax_code_format CHECK(tax_code IS NULL OR tax_code ~ '^[A-Z0-9][A-Z0-9_-]{0,39}$');
-- Commerce migrates before Accounting, so the tax codes are read late-bound
-- (as the book is in 011): an install without them has no code to name. The
-- code must be a confirmed output code of the company's book that applies on
-- the database's current date, the date tax codes are listed by.
CREATE FUNCTION archava_commerce.guard_order_tax() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE book uuid;
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
 -- A withdrawn version never applied, so it does not make a code known.
 IF NOT EXISTS(SELECT 1 FROM archava_accounting.tax_codes t WHERE t.tenant_id=NEW.tenant_id AND t.book_id=book AND t.code=NEW.tax_code AND t.state<>'withdrawn') THEN
  RAISE EXCEPTION 'Sales order tax code is not a tax code of the company book' USING ERRCODE='23514';END IF;
 IF EXISTS(SELECT 1 FROM archava_accounting.tax_codes t WHERE t.tenant_id=NEW.tenant_id AND t.book_id=book AND t.code=NEW.tax_code AND t.state<>'withdrawn' AND t.kind<>'output') THEN
  RAISE EXCEPTION 'A sales order needs an output tax code' USING ERRCODE='23514';END IF;
 IF EXISTS(SELECT 1 FROM archava_accounting.tax_codes t WHERE t.tenant_id=NEW.tenant_id AND t.book_id=book AND t.code=NEW.tax_code AND t.kind='output'
  AND t.state IN ('confirmed','retired') AND current_date BETWEEN t.effective_from AND coalesce(t.effective_to,DATE 'infinity')) THEN
  RETURN NEW;END IF;
 IF EXISTS(SELECT 1 FROM archava_accounting.tax_codes t WHERE t.tenant_id=NEW.tenant_id AND t.book_id=book AND t.code=NEW.tax_code AND t.state='entered' AND t.effective_from<=current_date) THEN
  RAISE EXCEPTION 'The sales order tax code is entered but not confirmed' USING ERRCODE='23514';END IF;
 RAISE EXCEPTION 'No confirmed version of the sales order tax code applies on the order date' USING ERRCODE='23514';
END $$;
CREATE TRIGGER a_tax_sales BEFORE INSERT OR UPDATE ON archava_commerce.orders FOR EACH ROW EXECUTE FUNCTION archava_commerce.guard_order_tax();
