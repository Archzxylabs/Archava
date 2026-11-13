-- Append-only strengthening of source/audit binding and startup capture readiness.
CREATE INDEX invoice_journal_source_lookup ON archava_accounting.journal_sources(tenant_id,source_id);
CREATE FUNCTION archava_accounting.assert_source_journal_claim() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE wanted uuid;j archava_accounting.journals;s uuid;op text;r jsonb;
BEGIN
 wanted:=coalesce((to_jsonb(NEW)->>'journal_id')::uuid,(to_jsonb(NEW)->>'id')::uuid);
 SELECT * INTO j FROM archava_accounting.journals WHERE tenant_id=NEW.tenant_id AND id=wanted;
 SELECT source_id INTO s FROM archava_accounting.journal_sources WHERE tenant_id=NEW.tenant_id AND journal_id=wanted;
 SELECT a.operation,c.receipt INTO op,r FROM archava_accounting.audit a JOIN archava_accounting.commands c ON c.tenant_id=a.tenant_id AND c.request_id=a.request_id
  WHERE a.tenant_id=NEW.tenant_id AND a.resource_kind='journal' AND a.resource_id=wanted AND a.revision=1;
 IF s IS NOT NULL THEN
  IF op IS DISTINCT FROM 'invoice_source.draft' OR r->>'sourceId' IS DISTINCT FROM s::text THEN
   RAISE EXCEPTION 'Source journal must retain its reviewed source command receipt' USING ERRCODE='23514';END IF;
 ELSIF op='invoice_source.draft' OR j.source_ref LIKE 'billing:%' THEN
  RAISE EXCEPTION 'The reserved billing source reference requires a bound source journal' USING ERRCODE='23514';
 END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER source_journal_claim_complete AFTER INSERT ON archava_accounting.journals DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_accounting.assert_source_journal_claim();
CREATE CONSTRAINT TRIGGER source_link_claim_complete AFTER INSERT ON archava_accounting.journal_sources DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION archava_accounting.assert_source_journal_claim();
CREATE FUNCTION archava_accounting.source_guards_complete() RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog AS $$
 SELECT NOT EXISTS(SELECT 1 FROM (VALUES
 ('archava_billing','commands','capture_financial_invoice_source','archava_accounting.capture_invoice_source()',17,false,false,'5',''),
 ('archava_accounting','invoice_policies','invoice_policy_guard','archava_accounting.guard_invoice_policy()',31,false,false,'',''),
 ('archava_accounting','invoice_policies','invoice_policy_complete','archava_accounting.assert_resource()',21,true,true,'','696e766f6963655f706f6c69637900'),
 ('archava_accounting','invoice_sources','invoice_source_guard','archava_accounting.guard_invoice_source()',7,false,false,'',''),
 ('archava_accounting','invoice_sources','invoice_source_immutable','archava_accounting.immutable()',27,false,false,'',''),
 ('archava_accounting','journal_sources','journal_source_guard','archava_accounting.guard_journal_source()',7,false,false,'',''),
 ('archava_accounting','journal_sources','journal_source_immutable','archava_accounting.immutable()',27,false,false,'',''),
 ('archava_accounting','periods','invoice_source_close_guard','archava_accounting.source_close_guard()',19,false,false,'',''),
 ('archava_accounting','journals','invoice_source_reversal_guard','archava_accounting.source_reversal_guard()',7,false,false,'',''),
 ('archava_accounting','journals','source_journal_claim_complete','archava_accounting.assert_source_journal_claim()',5,true,true,'',''),
 ('archava_accounting','journal_sources','source_link_claim_complete','archava_accounting.assert_source_journal_claim()',5,true,true,'','')
 ) AS required(schema_name,table_name,trigger_name,function_name,trigger_type,is_deferred,initially_deferred,columns,args_hex)
 WHERE NOT EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid=to_regclass(format('%I.%I',required.schema_name,required.table_name)) AND t.tgname=required.trigger_name
  AND t.tgfoid=to_regprocedure(required.function_name) AND t.tgtype=required.trigger_type AND t.tgenabled='O' AND NOT t.tgisinternal
  AND t.tgdeferrable=required.is_deferred AND t.tginitdeferred=required.initially_deferred AND t.tgqual IS NULL
  AND t.tgattr=required.columns::int2vector AND encode(t.tgargs,'hex')=required.args_hex))
$$;
REVOKE ALL ON FUNCTION archava_accounting.source_guards_complete() FROM PUBLIC;
