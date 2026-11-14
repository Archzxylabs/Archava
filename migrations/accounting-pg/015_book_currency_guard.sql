-- Trade currency: a company's trade currency is its book's currency, IDR while it
-- has no book, so every product, sales order, purchase order and invoice a company
-- made before its book is IDR. A book in another currency would strand them, so it
-- is refused while the company owns any such record. An IDR book is unaffected.
-- Books are immutable (001 book_immutable), so INSERT is the only path. Earlier SQL
-- bytes are unchanged, no row is rewritten and existing books are not re-examined.
DO $$ BEGIN
 PERFORM pg_advisory_xact_lock(hashtext('archava-serving-writer-takeover'));
 IF to_regclass('archava_control.writer_lease') IS NOT NULL THEN
  PERFORM 1 FROM archava_control.writer_lease WHERE lease_name='archava-serving' FOR UPDATE;
 END IF;
END $$;
-- The Commerce, Purchasing and Billing company guards lock the company row FOR SHARE
-- before their currency guards read the book. FOR NO KEY UPDATE conflicts with that
-- lock (and not with the KEY SHARE of foreign keys), so a trade insert and a non-IDR
-- book serialise on the company: whichever commits second sees the other. Each
-- statement below runs on a fresh READ COMMITTED snapshot taken after the lock.
-- An invoice made from an order and a supplier offer take no company lock, but they
-- need an existing company order or product, which already refuses the book.
CREATE FUNCTION archava_accounting.guard_book_currency() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF NEW.currency='IDR' THEN RETURN NEW;END IF;
 PERFORM 1 FROM archava_organizations.companies WHERE tenant_id=NEW.tenant_id AND id=NEW.company_id FOR NO KEY UPDATE;
 IF EXISTS(SELECT 1 FROM archava_commerce.products WHERE tenant_id=NEW.tenant_id AND company_id=NEW.company_id)
  OR EXISTS(SELECT 1 FROM archava_commerce.orders WHERE tenant_id=NEW.tenant_id AND company_id=NEW.company_id)
  OR EXISTS(SELECT 1 FROM archava_purchasing.orders WHERE tenant_id=NEW.tenant_id AND company_id=NEW.company_id)
  OR EXISTS(SELECT 1 FROM archava_billing.invoices WHERE tenant_id=NEW.tenant_id AND company_id=NEW.company_id) THEN
  RAISE EXCEPTION 'A company that already trades in IDR cannot open a book in another currency' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
-- Named to fire before 001 book_guard, whose FOR SHARE on the same company row is
-- then already covered: the stronger lock is taken first, never as an upgrade.
CREATE TRIGGER a_currency_book BEFORE INSERT ON archava_accounting.books FOR EACH ROW EXECUTE FUNCTION archava_accounting.guard_book_currency();
