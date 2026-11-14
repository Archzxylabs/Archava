-- Trade currency: a company-owned product or sales order takes the currency of
-- the company's accounting book; an unowned row, and a company without a book, is IDR.
-- Existing rows are not rewritten and currency stays immutable (001/002 guards).
ALTER TABLE archava_commerce.products DROP CONSTRAINT products_currency_check,ADD CONSTRAINT product_currency_code CHECK(currency ~ '^[A-Z]{3}$');
ALTER TABLE archava_commerce.orders DROP CONSTRAINT orders_currency_check,ADD CONSTRAINT sales_currency_code CHECK(currency ~ '^[A-Z]{3}$');
-- Commerce migrates before Accounting, so the book is read late-bound: an install
-- without the Accounting ledger has no book and therefore trades in IDR.
CREATE FUNCTION archava_commerce.trade_currency(t text,c uuid) RETURNS text LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE book_currency text;
BEGIN
 IF c IS NULL OR to_regclass('archava_accounting.books') IS NULL THEN RETURN 'IDR';END IF;
 SELECT currency INTO book_currency FROM archava_accounting.books WHERE tenant_id=t AND company_id=c;
 RETURN coalesce(book_currency,'IDR');
END $$;
CREATE FUNCTION archava_commerce.guard_trade_currency() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF NEW.currency IS DISTINCT FROM archava_commerce.trade_currency(NEW.tenant_id,NEW.company_id) THEN RAISE EXCEPTION 'Currency differs from the company book' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER a_currency_product BEFORE INSERT ON archava_commerce.products FOR EACH ROW EXECUTE FUNCTION archava_commerce.guard_trade_currency();
CREATE TRIGGER a_currency_sales BEFORE INSERT ON archava_commerce.orders FOR EACH ROW EXECUTE FUNCTION archava_commerce.guard_trade_currency();
CREATE FUNCTION archava_commerce.guard_line_currency() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE owner_currency text;item_currency text;
BEGIN
 SELECT currency INTO owner_currency FROM archava_commerce.orders WHERE tenant_id=NEW.tenant_id AND id=NEW.order_id;
 SELECT currency INTO item_currency FROM archava_commerce.products WHERE tenant_id=NEW.tenant_id AND id=NEW.product_id;
 IF owner_currency IS DISTINCT FROM item_currency THEN RAISE EXCEPTION 'Sales line is priced in another currency' USING ERRCODE='23514';END IF;RETURN NEW;
END $$;
CREATE TRIGGER a_currency_line BEFORE INSERT ON archava_commerce.order_lines FOR EACH ROW EXECUTE FUNCTION archava_commerce.guard_line_currency();
