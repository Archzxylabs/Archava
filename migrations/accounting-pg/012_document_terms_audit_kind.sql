-- Add the new immutable terms resource to the established audit vocabulary.
-- Accounting011 has already been exercised; preserve its bytes and append this fix.
DO $$ BEGIN
 PERFORM pg_advisory_xact_lock(hashtext('archava-serving-writer-takeover'));
 IF to_regclass('archava_control.writer_lease') IS NOT NULL THEN
  PERFORM 1 FROM archava_control.writer_lease WHERE lease_name='archava-serving' FOR UPDATE;
 END IF;
END $$;
ALTER TABLE archava_accounting.audit DROP CONSTRAINT audit_resource_kind_check;
ALTER TABLE archava_accounting.audit ADD CONSTRAINT audit_resource_kind_check CHECK(resource_kind IN
 ('book','account','period','journal','invoice_policy','purchase_policy','stock_policy','bank_account','bank_statement','bank_allocation','bank_unmatch','bank_policy','bank_reconciliation','document_terms'));
