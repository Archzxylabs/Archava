-- A taxed invoice keeps the tax ids of its seller and its buyer as they were
-- when the invoice was created, and can later be given the reference of a tax
-- invoice that was issued outside Archava. All of it is text the company
-- entered, stored as entered: Archava does not issue, validate or verify a tax
-- id or a tax invoice number, and talks to no tax authority.
--
-- seller_tax_id is the tax id of the invoice's managed company and
-- buyer_tax_id the tax id of its customer party, both at the moment of
-- creation and both immutable; null when the company or the party has none,
-- when the customer is not a business party, and on every untaxed invoice.
-- tax_reference is the externally issued number, tax_transaction_code the
-- company's own short classification of the transaction, and
-- tax_reference_version counts how often the reference was set: 0 without a
-- reference, 1 once recorded, one more for every replacement. The value a
-- replacement overwrote stays in the immutable billing audit row of the
-- command that replaced it. Existing rows get no identifiers, no reference and
-- version 0, and are not rewritten. No table is added and no grant changes.
ALTER TABLE archava_billing.invoices
 ADD COLUMN seller_tax_id text,ADD COLUMN buyer_tax_id text,
 ADD COLUMN tax_reference text,ADD COLUMN tax_transaction_code text,
 ADD COLUMN tax_reference_version integer NOT NULL DEFAULT 0,
 ADD CONSTRAINT invoice_tax_identifiers CHECK(
  (seller_tax_id IS NULL OR length(seller_tax_id) BETWEEN 1 AND 50) AND (buyer_tax_id IS NULL OR length(buyer_tax_id) BETWEEN 1 AND 50)
  AND (tax_code_id IS NOT NULL OR (seller_tax_id IS NULL AND buyer_tax_id IS NULL))),
 ADD CONSTRAINT invoice_tax_reference CHECK(
  (tax_reference IS NULL AND tax_transaction_code IS NULL AND tax_reference_version=0)
  OR (tax_code_id IS NOT NULL AND tax_reference_version>0 AND length(tax_reference) BETWEEN 1 AND 100 AND tax_reference=btrim(tax_reference)
   AND (tax_transaction_code IS NULL OR (length(tax_transaction_code) BETWEEN 1 AND 10 AND tax_transaction_code=btrim(tax_transaction_code)))));

-- Organizations migrates before Billing, but its rows are read through jsonb
-- so an install whose Organizations ledger predates the party tax id reads
-- "no tax id" (as Billing 009 reads the order's tax code).
CREATE FUNCTION archava_billing.guard_invoice_tax_identifiers() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE seller text;buyer text;
BEGIN
 IF TG_OP='INSERT' THEN
  IF NEW.tax_reference IS NOT NULL OR NEW.tax_transaction_code IS NOT NULL OR NEW.tax_reference_version<>0 THEN
   RAISE EXCEPTION 'A tax invoice reference can be recorded only on an issued or settled invoice' USING ERRCODE='23514';END IF;
  IF NEW.tax_code_id IS NOT NULL THEN
   SELECT to_jsonb(c)->>'tax_id' INTO seller FROM archava_organizations.companies c WHERE c.tenant_id=NEW.tenant_id AND c.id=NEW.company_id;
   IF NEW.customer_party_id IS NOT NULL THEN
    SELECT to_jsonb(p)->>'tax_id' INTO buyer FROM archava_organizations.parties p WHERE p.tenant_id=NEW.tenant_id AND p.id=NEW.customer_party_id;
   END IF;
  END IF;
  IF NEW.seller_tax_id IS DISTINCT FROM seller OR NEW.buyer_tax_id IS DISTINCT FROM buyer THEN
   RAISE EXCEPTION 'Invoice tax identifiers must be those of its company and customer party at creation' USING ERRCODE='23514';END IF;
  RETURN NEW;
 END IF;
 IF NEW.seller_tax_id IS DISTINCT FROM OLD.seller_tax_id OR NEW.buyer_tax_id IS DISTINCT FROM OLD.buyer_tax_id THEN
  RAISE EXCEPTION 'Invoice tax identifier snapshot is immutable' USING ERRCODE='23514';END IF;
 IF (NEW.tax_reference,NEW.tax_transaction_code,NEW.tax_reference_version) IS NOT DISTINCT FROM (OLD.tax_reference,OLD.tax_transaction_code,OLD.tax_reference_version) THEN
  RETURN NEW;END IF;
 IF NEW.tax_code_id IS NULL THEN
  RAISE EXCEPTION 'Only a taxed invoice can carry a tax invoice reference' USING ERRCODE='23514';END IF;
 IF OLD.status NOT IN ('issued','settled') OR NEW.status IS DISTINCT FROM OLD.status THEN
  RAISE EXCEPTION 'A tax invoice reference can be recorded only on an issued or settled invoice' USING ERRCODE='23514';END IF;
 IF NEW.tax_reference IS NULL THEN
  RAISE EXCEPTION 'A recorded tax invoice reference cannot be removed' USING ERRCODE='23514';END IF;
 IF OLD.tax_reference IS NOT NULL AND NEW.tax_reference_version=1 THEN
  RAISE EXCEPTION 'The invoice already has a tax invoice reference' USING ERRCODE='23514';END IF;
 IF OLD.tax_reference IS NULL AND NEW.tax_reference_version<>1 THEN
  RAISE EXCEPTION 'The invoice has no tax invoice reference to replace' USING ERRCODE='23514';END IF;
 IF NEW.tax_reference_version<>OLD.tax_reference_version+1 OR NEW.revision<>OLD.revision+1
  OR (NEW.correction_minor,NEW.settled_minor,NEW.outstanding_minor) IS DISTINCT FROM (OLD.correction_minor,OLD.settled_minor,OLD.outstanding_minor) THEN
  RAISE EXCEPTION 'A tax invoice reference changes alone, one version at a time' USING ERRCODE='23514';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER invoice_tax_identifiers_guard BEFORE INSERT OR UPDATE ON archava_billing.invoices FOR EACH ROW EXECUTE FUNCTION archava_billing.guard_invoice_tax_identifiers();
