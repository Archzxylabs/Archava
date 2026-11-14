-- Tax review (2026-11-14), Purchasing part. A taxed supplier document is owed
-- in gross: its goods total plus its tax. When the claim is matched the
-- accounting bridge stores that gross in ledger minor units and refuses one
-- above 9,000,000,000,000,000 (Accounting 018), which until now happened only
-- at the match, after the claim had been stored. A captured version, a new
-- claim or a correction, is now held to the same bound when it is written.
--
-- The unit factor is the posting policy's, which a document does not have,
-- but the policy's unit is fixed by the currency: whole Rupiah (x100) is the
-- only unit of an IDR book and the minor unit (x1) the only unit of any other
-- (Accounting 014), and a company without a book trades in IDR. So the gross
-- is bounded by 90,000,000,000,000 for an IDR document and by
-- 9,000,000,000,000,000 otherwise, whether or not the order posts purchases.
-- Only a document with a tax code is checked: an untaxed claim keeps the
-- bounds it had. guard_supplier_document_tax() of 008 is replaced in place
-- with its signature, so the trigger supplier_document_tax_guard that calls it
-- is unchanged; every other rule is the same. A stored version is not looked
-- at again, no row is rewritten and no grant changes.
CREATE OR REPLACE FUNCTION archava_purchasing.guard_supplier_document_tax() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE book uuid;applicable record;
BEGIN
 IF TG_OP='UPDATE' AND NEW.state<>'captured' THEN
  IF (NEW.tax_minor,NEW.tax_computed_minor,NEW.tax_code_id,NEW.tax_code,NEW.tax_code_version,NEW.tax_rate_numerator,NEW.tax_rate_denominator,NEW.tax_base_numerator,NEW.tax_base_denominator)
   IS DISTINCT FROM (OLD.tax_minor,OLD.tax_computed_minor,OLD.tax_code_id,OLD.tax_code,OLD.tax_code_version,OLD.tax_rate_numerator,OLD.tax_rate_denominator,OLD.tax_base_numerator,OLD.tax_base_denominator) THEN
   RAISE EXCEPTION 'Supplier document tax changes only with a corrected claim version' USING ERRCODE='23514';END IF;
  RETURN NEW;
 END IF;
 IF NEW.tax_code IS NULL THEN
  IF NEW.tax_minor IS DISTINCT FROM 0 OR NEW.tax_computed_minor IS DISTINCT FROM 0 OR NEW.tax_code_id IS NOT NULL OR NEW.tax_code_version IS NOT NULL
   OR NEW.tax_rate_numerator IS NOT NULL OR NEW.tax_rate_denominator IS NOT NULL OR NEW.tax_base_numerator IS NOT NULL OR NEW.tax_base_denominator IS NOT NULL THEN
   RAISE EXCEPTION 'A supplier document tax code and its tax amount are stated together' USING ERRCODE='23514';END IF;
  RETURN NEW;
 END IF;
 IF NEW.tax_minor IS NULL THEN
  RAISE EXCEPTION 'A supplier document tax code and its tax amount are stated together' USING ERRCODE='23514';END IF;
 IF to_regclass('archava_accounting.tax_codes') IS NOT NULL THEN
  SELECT id INTO book FROM archava_accounting.books WHERE tenant_id=NEW.tenant_id AND company_id=NEW.company_id;
 END IF;
 IF book IS NULL THEN
  RAISE EXCEPTION 'Supplier document tax code is not a tax code of the company book' USING ERRCODE='23514';END IF;
 -- A withdrawn version never applied, so it does not make a code known.
 IF NOT EXISTS(SELECT 1 FROM archava_accounting.tax_codes t WHERE t.tenant_id=NEW.tenant_id AND t.book_id=book AND t.code=NEW.tax_code AND t.state<>'withdrawn') THEN
  RAISE EXCEPTION 'Supplier document tax code is not a tax code of the company book' USING ERRCODE='23514';END IF;
 IF EXISTS(SELECT 1 FROM archava_accounting.tax_codes t WHERE t.tenant_id=NEW.tenant_id AND t.book_id=book AND t.code=NEW.tax_code AND t.state<>'withdrawn' AND t.kind<>'input') THEN
  RAISE EXCEPTION 'A supplier document needs an input tax code' USING ERRCODE='23514';END IF;
 SELECT t.id,t.version,t.rate_numerator,t.rate_denominator,t.base_numerator,t.base_denominator INTO applicable FROM archava_accounting.tax_codes t
  WHERE t.tenant_id=NEW.tenant_id AND t.book_id=book AND t.code=NEW.tax_code AND t.kind='input' AND t.state IN ('confirmed','retired')
  AND NEW.document_date BETWEEN t.effective_from AND coalesce(t.effective_to,DATE 'infinity');
 IF applicable.id IS NULL THEN
  IF EXISTS(SELECT 1 FROM archava_accounting.tax_codes t WHERE t.tenant_id=NEW.tenant_id AND t.book_id=book AND t.code=NEW.tax_code AND t.state='entered' AND t.effective_from<=NEW.document_date) THEN
   RAISE EXCEPTION 'The supplier document tax code is entered but not confirmed' USING ERRCODE='23514';END IF;
  RAISE EXCEPTION 'No confirmed version of the supplier document tax code applies on the document date' USING ERRCODE='23514';
 END IF;
 IF (NEW.tax_code_id,NEW.tax_code_version,NEW.tax_rate_numerator,NEW.tax_rate_denominator,NEW.tax_base_numerator,NEW.tax_base_denominator)
  IS DISTINCT FROM (applicable.id,applicable.version,applicable.rate_numerator,applicable.rate_denominator,applicable.base_numerator,applicable.base_denominator) THEN
  RAISE EXCEPTION 'Supplier document tax snapshot differs from the tax code version that applies on the document date' USING ERRCODE='23514';END IF;
 IF NEW.tax_minor NOT BETWEEN 0 AND NEW.declared_total THEN
  RAISE EXCEPTION 'A supplier document tax amount cannot be negative or exceed its goods total' USING ERRCODE='23514';END IF;
 IF (NEW.declared_total::numeric+NEW.tax_minor)*(CASE WHEN NEW.currency='IDR' THEN 100 ELSE 1 END)>9000000000000000 THEN
  RAISE EXCEPTION 'A taxed supplier document total exceeds the amount the ledger can post' USING ERRCODE='23514';END IF;
 IF NEW.tax_computed_minor IS DISTINCT FROM archava_purchasing.tax_on_net(NEW.declared_total,NEW.tax_rate_numerator,NEW.tax_rate_denominator,NEW.tax_base_numerator,NEW.tax_base_denominator) THEN
  RAISE EXCEPTION 'Supplier document computed tax must follow the stored tax rule' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
