-- Tax stage D: a supplier document may carry one input tax code of its
-- company's book and the tax amount printed on the supplier's document.
-- declared_total and the claimed line prices stay the net of goods; the amount
-- owed to the supplier is declared_total + tax_minor. The document stores the
-- tax code version that applies on its own document_date (code id, code,
-- version, rate and base fraction), the entered tax (tax_minor) and the tax the
-- code gives on the net goods total (tax_computed_minor). The entered amount is
-- the one owed; it is not required to equal the computed one. Every version
-- snapshot is the whole row (006), so each version keeps its own tax.
-- Matching is unchanged: tax plays no part in whether a claim matches.
-- Existing rows are untaxed (no snapshot, tax 0) and are not rewritten.
-- Nothing here posts tax to a ledger; Accounting 018 does.
ALTER TABLE archava_purchasing.supplier_documents
 ADD COLUMN tax_minor bigint NOT NULL DEFAULT 0,ADD COLUMN tax_computed_minor bigint NOT NULL DEFAULT 0,
 ADD COLUMN tax_code_id uuid,ADD COLUMN tax_code text,ADD COLUMN tax_code_version integer,
 ADD COLUMN tax_rate_numerator integer,ADD COLUMN tax_rate_denominator integer,
 ADD COLUMN tax_base_numerator integer,ADD COLUMN tax_base_denominator integer,
 ADD CONSTRAINT supplier_document_tax_snapshot CHECK((
  (tax_code_id IS NULL AND tax_code IS NULL AND tax_code_version IS NULL
   AND tax_rate_numerator IS NULL AND tax_rate_denominator IS NULL AND tax_base_numerator IS NULL AND tax_base_denominator IS NULL
   AND tax_minor=0 AND tax_computed_minor=0)
  OR (tax_code_id IS NOT NULL AND tax_code IS NOT NULL AND tax_code_version>0
   AND tax_rate_denominator BETWEEN 1 AND 1000000 AND tax_rate_numerator BETWEEN 0 AND tax_rate_denominator
   AND tax_base_denominator BETWEEN 1 AND 1000000 AND tax_base_numerator BETWEEN 1 AND tax_base_denominator
   AND tax_minor BETWEEN 0 AND declared_total AND tax_computed_minor BETWEEN 0 AND declared_total)) IS TRUE);

-- Integer tax arithmetic (TAX.md rules 4 and 5), the same as the domain's and
-- Billing 009's, held in this ledger so the guard does not depend on how far
-- Billing has migrated: a quotient rounded half away from zero and the tax a
-- net amount carries. numeric keeps every intermediate exact.
CREATE FUNCTION archava_purchasing.tax_round(n numeric,d numeric) RETURNS bigint LANGUAGE sql IMMUTABLE SET search_path=pg_catalog AS $$
 SELECT (sign(n)*div(2*abs(n)+d,2*d))::bigint
$$;
CREATE FUNCTION archava_purchasing.tax_on_net(net bigint,rate_n integer,rate_d integer,base_n integer,base_d integer) RETURNS bigint LANGUAGE sql IMMUTABLE SET search_path=pg_catalog AS $$
 SELECT archava_purchasing.tax_round(net::numeric*(base_n::numeric*rate_n),base_d::numeric*rate_d)
$$;

-- The tax of a document is entered with a captured version (a new claim or a
-- correction) and is fixed for every later version of that claim: a match, a
-- hold and a void keep it. A captured version that names a tax code stores the
-- confirmed input version of the company's book that applies on the document
-- date, exactly as that version reads, and the tax that version gives on the
-- net goods total. Purchasing migrates before Accounting, so the tax codes are
-- read late-bound (as the book is in 007): an install without them has no code
-- to name.
CREATE FUNCTION archava_purchasing.guard_supplier_document_tax() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
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
 IF NEW.tax_computed_minor IS DISTINCT FROM archava_purchasing.tax_on_net(NEW.declared_total,NEW.tax_rate_numerator,NEW.tax_rate_denominator,NEW.tax_base_numerator,NEW.tax_base_denominator) THEN
  RAISE EXCEPTION 'Supplier document computed tax must follow the stored tax rule' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER supplier_document_tax_guard BEFORE INSERT OR UPDATE ON archava_purchasing.supplier_documents FOR EACH ROW EXECUTE FUNCTION archava_purchasing.guard_supplier_document_tax();
