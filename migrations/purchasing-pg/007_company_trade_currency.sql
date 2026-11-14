-- Trade currency: a purchase order takes the currency of its company's accounting
-- book, a supplier offer that of the company owning its product; a row without a
-- company, and a company without a book, is IDR. A supplier document already carries
-- its purchase order's currency (006 guard). Existing rows are not rewritten and
-- currency stays immutable (001/006 guards).
ALTER TABLE archava_purchasing.offers DROP CONSTRAINT offers_currency_check,ADD CONSTRAINT offer_currency_code CHECK(currency ~ '^[A-Z]{3}$');
ALTER TABLE archava_purchasing.orders DROP CONSTRAINT orders_currency_check,ADD CONSTRAINT purchase_currency_code CHECK(currency ~ '^[A-Z]{3}$');
ALTER TABLE archava_purchasing.supplier_documents DROP CONSTRAINT supplier_documents_currency_check,ADD CONSTRAINT supplier_document_currency_code CHECK(currency ~ '^[A-Z]{3}$');
-- The rule of archava_commerce.trade_currency (Commerce 011), held in this ledger so
-- these guards do not depend on how far Commerce has migrated. Purchasing migrates
-- before Accounting, so the book is read late-bound: an install without the
-- Accounting ledger has no book and therefore trades in IDR.
CREATE FUNCTION archava_purchasing.trade_currency(t text,c uuid) RETURNS text LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE book_currency text;
BEGIN
 IF c IS NULL OR to_regclass('archava_accounting.books') IS NULL THEN RETURN 'IDR';END IF;
 SELECT currency INTO book_currency FROM archava_accounting.books WHERE tenant_id=t AND company_id=c;
 RETURN coalesce(book_currency,'IDR');
END $$;
CREATE FUNCTION archava_purchasing.guard_order_currency() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF NEW.currency IS DISTINCT FROM archava_purchasing.trade_currency(NEW.tenant_id,NEW.company_id) THEN RAISE EXCEPTION 'Currency differs from the company book' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER a_currency_order BEFORE INSERT ON archava_purchasing.orders FOR EACH ROW EXECUTE FUNCTION archava_purchasing.guard_order_currency();
-- An offer has no company of its own: it is priced for one product, in that
-- product's currency, which must still be its company's trade currency.
CREATE FUNCTION archava_purchasing.guard_offer_currency() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE item_company uuid;item_currency text;
BEGIN
 SELECT company_id,currency INTO item_company,item_currency FROM archava_commerce.products WHERE tenant_id=NEW.tenant_id AND id=NEW.product_id;
 IF NEW.currency IS DISTINCT FROM archava_purchasing.trade_currency(NEW.tenant_id,item_company) THEN RAISE EXCEPTION 'Currency differs from the company book' USING ERRCODE='23514';END IF;
 IF NEW.currency IS DISTINCT FROM item_currency THEN RAISE EXCEPTION 'Offer is priced in another currency than its product' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER a_currency_offer BEFORE INSERT ON archava_purchasing.offers FOR EACH ROW EXECUTE FUNCTION archava_purchasing.guard_offer_currency();
CREATE FUNCTION archava_purchasing.guard_line_currency() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE owner_currency text;item_currency text;offer_currency text;
BEGIN
 SELECT currency INTO owner_currency FROM archava_purchasing.orders WHERE tenant_id=NEW.tenant_id AND id=NEW.order_id;
 SELECT currency INTO item_currency FROM archava_commerce.products WHERE tenant_id=NEW.tenant_id AND id=NEW.product_id;
 SELECT currency INTO offer_currency FROM archava_purchasing.offers WHERE tenant_id=NEW.tenant_id AND id=NEW.offer_id;
 IF owner_currency IS DISTINCT FROM item_currency OR owner_currency IS DISTINCT FROM offer_currency THEN RAISE EXCEPTION 'Purchase line is priced in another currency' USING ERRCODE='23514';END IF;RETURN NEW;
END $$;
CREATE TRIGGER a_currency_line BEFORE INSERT ON archava_purchasing.lines FOR EACH ROW EXECUTE FUNCTION archava_purchasing.guard_line_currency();
