/** Actual previous distribution -> current native migration, replay, and isolated rollback. */
import assert from 'node:assert/strict'
import {randomBytes,randomUUID} from 'node:crypto'
import {existsSync,readFileSync} from 'node:fs'
import {join,resolve} from 'node:path'
import {spawnSync} from 'node:child_process'
import {verifyNativePayables} from './native-payables.mjs'

export async function verifyPriorNative({previousRoot,packageRoot,facts,scratch,productionEnvironment,startServer,terminate,until,freePort,pass,requireDisclosureMigration=false}){
 const previous=resolve(previousRoot),current=resolve(packageRoot)
 assert.notEqual(previous,current);assert.ok(existsSync(join(previous,'bin/archava-server')))
 const oldManifest=JSON.parse(readFileSync(join(previous,'migrations/manifest.v1.json'),'utf8')),newManifest=JSON.parse(readFileSync(join(current,'migrations/manifest.v1.json'),'utf8'))
 for(const old of oldManifest.ledgers){const next=newManifest.ledgers.find(l=>l.namespace===old.namespace);assert.ok(next,`Previous ledger preserved: ${old.namespace}`);for(let i=0;i<old.migrations.length;i++){assert.equal(next.migrations[i]?.id,old.migrations[i].id);assert.equal(next.migrations[i]?.sha256,old.migrations[i].sha256)}}
 const expectedAdded=newManifest.ledgers.reduce((n,l)=>n+l.migrations.length,0)-oldManifest.ledgers.reduce((n,l)=>n+l.migrations.length,0)
 const priorAccounting=oldManifest.ledgers.some(l=>l.namespace==='archava_accounting')
 const priorInvoiceSources=oldManifest.ledgers.find(l=>l.namespace==='archava_accounting')?.migrations.some(m=>m.id==='002_invoice_source_posting')===true
 const priorPurchaseSources=oldManifest.ledgers.find(l=>l.namespace==='archava_accounting')?.migrations.some(m=>m.id==='004_purchase_source_posting')===true
 const priorStockSources=oldManifest.ledgers.find(l=>l.namespace==='archava_accounting')?.migrations.some(m=>m.id==='005_stock_source_posting')===true
 const bankMigrationIds=['006_manual_bank_evidence','007_bank_cash_matching','008_reviewed_bank_periods']
 const bankTables=['bank_accounts','bank_statements','bank_rows','bank_allocations','bank_unmatches','bank_policies','bank_reconciliations']
 const newBank=newManifest.ledgers.find(l=>l.namespace==='archava_accounting')?.migrations.some(m=>m.id==='008_reviewed_bank_periods')===true
 const addedBank=newBank&&oldManifest.ledgers.find(l=>l.namespace==='archava_accounting')?.migrations.some(m=>m.id==='008_reviewed_bank_periods')!==true
 const newPayables=newManifest.ledgers.find(l=>l.namespace==='archava_accounting')?.migrations.some(m=>m.id==='009_supplier_bank_settlements')===true
 const oldPayables=oldManifest.ledgers.find(l=>l.namespace==='archava_accounting')?.migrations.some(m=>m.id==='009_supplier_bank_settlements')===true
 const newClearing=newManifest.ledgers.find(l=>l.namespace==='archava_accounting')?.migrations.some(m=>m.id==='010_gateway_bank_settlements')===true
 const oldClearing=oldManifest.ledgers.find(l=>l.namespace==='archava_accounting')?.migrations.some(m=>m.id==='010_gateway_bank_settlements')===true
 const termsMigrationIds=['011_reviewed_document_terms','012_document_terms_audit_kind']
 const newTerms=termsMigrationIds.every(id=>newManifest.ledgers.find(l=>l.namespace==='archava_accounting')?.migrations.some(m=>m.id===id))
 const oldTerms=oldManifest.ledgers.find(l=>l.namespace==='archava_accounting')?.migrations.some(m=>m.id==='011_reviewed_document_terms')===true
 const newOpenings=newManifest.ledgers.find(l=>l.namespace==='archava_accounting')?.migrations.some(m=>m.id==='013_declared_opening_balances')===true
 const oldOpenings=oldManifest.ledgers.find(l=>l.namespace==='archava_accounting')?.migrations.some(m=>m.id==='013_declared_opening_balances')===true
 const sourceUnit=manifest=>manifest.ledgers.find(l=>l.namespace==='archava_accounting')?.migrations.some(m=>m.id==='014_source_unit_minor')===true
 const bookGuard=manifest=>manifest.ledgers.find(l=>l.namespace==='archava_accounting')?.migrations.some(m=>m.id==='015_book_currency_guard')===true
 const taxCodes=manifest=>manifest.ledgers.find(l=>l.namespace==='archava_accounting')?.migrations.some(m=>m.id==='016_tax_codes')===true
 const tradeCurrency=manifest=>manifest.ledgers.find(l=>l.namespace==='archava_commerce')?.migrations.some(m=>m.id==='011_company_trade_currency')===true
 const salesTax=manifest=>manifest.ledgers.find(l=>l.namespace==='archava_commerce')?.migrations.some(m=>m.id==='012_sales_order_tax')===true
 const salesTaxUtc=manifest=>manifest.ledgers.find(l=>l.namespace==='archava_commerce')?.migrations.some(m=>m.id==='013_sales_order_tax_utc_date')===true
 const commerceAdded=[...(tradeCurrency(newManifest)&&!tradeCurrency(oldManifest)?['011_company_trade_currency']:[]),...(salesTax(newManifest)&&!salesTax(oldManifest)?['012_sales_order_tax']:[]),...(salesTaxUtc(newManifest)&&!salesTaxUtc(oldManifest)?['013_sales_order_tax_utc_date']:[])]
 const invoiceTax=manifest=>manifest.ledgers.find(l=>l.namespace==='archava_billing')?.migrations.some(m=>m.id==='009_invoice_tax_snapshot')===true
 const taxPosting=manifest=>manifest.ledgers.find(l=>l.namespace==='archava_billing')?.migrations.some(m=>m.id==='010_taxed_invoice_posting')===true
 const sourceTax=manifest=>manifest.ledgers.find(l=>l.namespace==='archava_accounting')?.migrations.some(m=>m.id==='017_invoice_source_tax')===true
 const invoiceIdentifiers=manifest=>manifest.ledgers.find(l=>l.namespace==='archava_billing')?.migrations.some(m=>m.id==='011_invoice_tax_identifiers')===true
 const billingTaxReview=manifest=>manifest.ledgers.find(l=>l.namespace==='archava_billing')?.migrations.some(m=>m.id==='012_tax_review_guards')===true
 const manualReceipts=manifest=>manifest.ledgers.find(l=>l.namespace==='archava_billing')?.migrations.some(m=>m.id==='013_manual_customer_receipts')===true
 const manualReceiptSources=manifest=>manifest.ledgers.find(l=>l.namespace==='archava_accounting')?.migrations.some(m=>m.id==='020_manual_receipt_sources')===true
 const billingAdded=[...(invoiceTax(newManifest)&&!invoiceTax(oldManifest)?['009_invoice_tax_snapshot']:[]),...(taxPosting(newManifest)&&!taxPosting(oldManifest)?['010_taxed_invoice_posting']:[]),...(invoiceIdentifiers(newManifest)&&!invoiceIdentifiers(oldManifest)?['011_invoice_tax_identifiers']:[]),...(billingTaxReview(newManifest)&&!billingTaxReview(oldManifest)?['012_tax_review_guards']:[]),...(manualReceipts(newManifest)&&!manualReceipts(oldManifest)?['013_manual_customer_receipts']:[])]
 const taxRegistration=manifest=>manifest.ledgers.find(l=>l.namespace==='archava_organizations')?.migrations.some(m=>m.id==='005_tax_registration')===true
 const organizationsAdded=taxRegistration(newManifest)&&!taxRegistration(oldManifest)?['005_tax_registration']:[]
 const purchaseCurrency=manifest=>manifest.ledgers.find(l=>l.namespace==='archava_purchasing')?.migrations.some(m=>m.id==='007_company_trade_currency')===true
 const documentTax=manifest=>manifest.ledgers.find(l=>l.namespace==='archava_purchasing')?.migrations.some(m=>m.id==='008_supplier_document_tax')===true
 const purchaseSourceTax=manifest=>manifest.ledgers.find(l=>l.namespace==='archava_accounting')?.migrations.some(m=>m.id==='018_purchase_source_tax')===true
 const documentTaxBound=manifest=>manifest.ledgers.find(l=>l.namespace==='archava_purchasing')?.migrations.some(m=>m.id==='009_supplier_document_tax_bound')===true
 const accountingTaxReview=manifest=>manifest.ledgers.find(l=>l.namespace==='archava_accounting')?.migrations.some(m=>m.id==='019_tax_review_guards')===true
 const purchasingAdded=[...(purchaseCurrency(newManifest)&&!purchaseCurrency(oldManifest)?['007_company_trade_currency']:[]),...(documentTax(newManifest)&&!documentTax(oldManifest)?['008_supplier_document_tax']:[]),...(documentTaxBound(newManifest)&&!documentTaxBound(oldManifest)?['009_supplier_document_tax_bound']:[])]
 const oldDisclosure=oldManifest.ledgers.find(l=>l.namespace==='archava_owner')?.migrations.some(m=>m.id==='004_ai_disclosure_boundary')===true
 const newDisclosure=newManifest.ledgers.find(l=>l.namespace==='archava_owner')?.migrations.some(m=>m.id==='004_ai_disclosure_boundary')===true
 assert.ok(expectedAdded>=0)
 if(requireDisclosureMigration){
  assert.equal(oldDisclosure,false);assert.equal(newDisclosure,true)
  const oldOwner=oldManifest.ledgers.find(l=>l.namespace==='archava_owner'),newOwner=newManifest.ledgers.find(l=>l.namespace==='archava_owner')
  assert.deepEqual(newOwner.migrations.slice(oldOwner.migrations.length).map(m=>m.id),['004_ai_disclosure_boundary'])
  const oldAccounting=oldManifest.ledgers.find(l=>l.namespace==='archava_accounting'),newAccounting=newManifest.ledgers.find(l=>l.namespace==='archava_accounting')
  const accountingAdded=[...(addedBank?bankMigrationIds:[]),...(newPayables&&!oldPayables?['009_supplier_bank_settlements']:[]),...(newClearing&&!oldClearing?['010_gateway_bank_settlements']:[]),...(newTerms&&!oldTerms?termsMigrationIds:[]),...(newOpenings&&!oldOpenings?['013_declared_opening_balances']:[]),...(sourceUnit(newManifest)&&!sourceUnit(oldManifest)?['014_source_unit_minor']:[]),...(bookGuard(newManifest)&&!bookGuard(oldManifest)?['015_book_currency_guard']:[]),...(taxCodes(newManifest)&&!taxCodes(oldManifest)?['016_tax_codes']:[]),...(sourceTax(newManifest)&&!sourceTax(oldManifest)?['017_invoice_source_tax']:[]),...(purchaseSourceTax(newManifest)&&!purchaseSourceTax(oldManifest)?['018_purchase_source_tax']:[]),...(accountingTaxReview(newManifest)&&!accountingTaxReview(oldManifest)?['019_tax_review_guards']:[]),...(manualReceiptSources(newManifest)&&!manualReceiptSources(oldManifest)?['020_manual_receipt_sources']:[])]
  assert.deepEqual(newAccounting.migrations.slice(oldAccounting.migrations.length).map(m=>m.id),accountingAdded)
  const oldCommerce=oldManifest.ledgers.find(l=>l.namespace==='archava_commerce'),newCommerce=newManifest.ledgers.find(l=>l.namespace==='archava_commerce')
  assert.deepEqual(newCommerce.migrations.slice(oldCommerce.migrations.length).map(m=>m.id),commerceAdded)
  const oldPurchasing=oldManifest.ledgers.find(l=>l.namespace==='archava_purchasing'),newPurchasing=newManifest.ledgers.find(l=>l.namespace==='archava_purchasing')
  assert.deepEqual(newPurchasing.migrations.slice(oldPurchasing.migrations.length).map(m=>m.id),purchasingAdded)
  const oldBilling=oldManifest.ledgers.find(l=>l.namespace==='archava_billing'),newBilling=newManifest.ledgers.find(l=>l.namespace==='archava_billing')
  assert.deepEqual(newBilling.migrations.slice(oldBilling.migrations.length).map(m=>m.id),billingAdded)
  const oldOrganizations=oldManifest.ledgers.find(l=>l.namespace==='archava_organizations'),newOrganizations=newManifest.ledgers.find(l=>l.namespace==='archava_organizations')
  assert.deepEqual(newOrganizations.migrations.slice(oldOrganizations.migrations.length).map(m=>m.id),organizationsAdded)
  assert.equal(expectedAdded,1+accountingAdded.length+commerceAdded.length+purchasingAdded.length+billingAdded.length+organizationsAdded.length,'This prior profile adds exactly the Owner disclosure and the declared bank/payable, trade-currency and tax migrations')
 }
 const db=`archava_finupgrade_${randomUUID().slice(0,8)}`,rollback=`${db}_rollback`,fresh=`${db}_fresh`
 assert.match(db,/^archava_finupgrade_[0-9a-f]{8}$/)
 const password='correct horse battery staple',username='native-owner@archava.test',tenant='native-upgrade'
 const backup=join(scratch,`${db}-prior-native-backup`),backupKey=randomBytes(32).toString('hex')
 const sql=(database,input)=>{const r=spawnSync('psql',['-X','-qAt','-v','ON_ERROR_STOP=1','-h','127.0.0.1','-p',String(facts.port),'-U','postgres','-d',database],{env:{...process.env,PGPASSWORD:'archava'},input,encoding:'utf8',timeout:30000});assert.equal(r.status,0,r.stderr);return r.stdout.trim()}
 const ops=(root,database,args)=>{
  const r=spawnSync(join(root,'bin/archava-ops'),args,{cwd:root,env:{...process.env,ARCHAVA_OPS_ROOT:root,ARCHAVA_MIGRATION_DATABASE_URL:`postgres://archava_owner:archava@127.0.0.1:${facts.port}/${database}`,ARCHAVA_OWNER_PASSWORD:password,ARCHAVA_BACKUP_KEY:backupKey,PGPASSWORD:'archava'},encoding:'utf8',timeout:55000,maxBuffer:4*1024*1024})
  assert.equal(r.status,0,`${r.stderr}\n${r.stdout}`);const body=JSON.parse(r.stdout);assert.equal(body.ok,true);return body
 }
 const priorFinancialTables=['memberships','books','accounts','periods','journals','journal_lines','commands','audit',...(priorInvoiceSources?['invoice_policies','invoice_sources','journal_sources']:[]),...(priorPurchaseSources?['purchase_policies','purchase_origins','purchase_sources','purchase_journal_sources']:[]),...(priorStockSources?['stock_policies','stock_layer_origins','stock_sources','stock_journal_sources']:[])]
 // Accounting 017 adds tax columns to invoice_sources and 018 the same three to purchase_sources. The digest leaves them out so a prior row compares equal across the upgrade; they must arrive empty (asserted below).
 const sourceTaxColumns=['tax_minor','tax_account_id','tax_account_revision']
 const proofRow=table=>table==='invoice_sources'||table==='purchase_sources'?`(to_jsonb(r)-ARRAY[${sourceTaxColumns.map(c=>`'${c}'`).join(',')}])`:'to_jsonb(r)'
 const financialProof=priorAccounting?`jsonb_build_object(${priorFinancialTables.map(table=>`'${table}',(SELECT jsonb_build_object('rows',count(*),'digest',md5(coalesce(string_agg(${proofRow(table)}::text,'' ORDER BY ${proofRow(table)}::text),''))) FROM archava_accounting.${table} r)`).join(',')})`:'NULL'
 // Organizations 005 adds vat_registered to companies. The proof leaves it out so a prior company compares equal across the upgrade; it must arrive false (asserted below).
 const proof=database=>JSON.parse(sql(database,`SELECT jsonb_build_object(
  'users',(SELECT jsonb_build_object('rows',count(*),'digest',md5(coalesce(string_agg(to_jsonb(r)::text,'' ORDER BY id),''))) FROM archava_identity.users r),
  'organizationMemberships',(SELECT jsonb_agg(to_jsonb(r) ORDER BY tenant_id,principal_id) FROM archava_organizations.memberships r),
  'organizations',(SELECT jsonb_agg(to_jsonb(r) ORDER BY tenant_id,id) FROM archava_organizations.organizations r),
  'companies',(SELECT jsonb_agg(to_jsonb(r)-'vat_registered' ORDER BY tenant_id,id) FROM archava_organizations.companies r),
  'products',(SELECT jsonb_agg(to_jsonb(r) ORDER BY tenant_id,id) FROM archava_commerce.products r),
  'commands',(SELECT jsonb_agg(to_jsonb(r) ORDER BY tenant_id,request_id) FROM archava_commerce.commands r),
  'accounting',${financialProof});`))
 const start=async(root,database,name)=>{
  const port=await freePort(),origin=`http://127.0.0.1:${port}`
  const env={...productionEnvironment,DATABASE_URL:`postgres://archava_runtime:archava@127.0.0.1:${facts.port}/${database}`,ARCHAVA_BIND_ADDR:`127.0.0.1:${port}`,ARCHAVA_SITE_ROOT:join(root,'site'),ARCHAVA_WRITER_TAKEOVER:'false',GEMINI_AGENT_MODEL:'',GEMINI_AGENT_API_KEY:'',GEMINI_API_KEY:''}
  for(const key of ['ARCHAVA_PAYMENT_CONFIG','ARCHAVA_EMAIL_CONFIG','ARCHAVA_AGENT_CONFIG'])env[key]=undefined
  const child=startServer(join(root,'bin/archava-server'),env,name)
  try{await until(async()=>{try{return(await fetch(`${origin}/readyz`)).status===200}catch{return false}},name+' ready');return{child,origin}}catch(e){await terminate(child);throw e}
 }
 const login=async origin=>{
  const r=await fetch(`${origin}/login`,{method:'POST',headers:{'content-type':'application/json','x-archava-client':'cli'},body:JSON.stringify({username,password})});assert.equal(r.status,200);return r.json()
 }
 const call=async(origin,session,path,body)=>{
  const r=await fetch(`${origin}${path}`,{method:body?'POST':'GET',headers:{'content-type':'application/json','x-archava-client':'cli',authorization:`Bearer ${session.token}`,'x-archava-csrf':session.csrf},...(body?{body:JSON.stringify(body)}:{})});return{status:r.status,body:await r.json()}
 }
 let oldServer,newServer,rollbackServer,freshServer,created=false,rollbackCreated=false,freshCreated=false,priorBook=null,priorSourceBook=null,priorPurchaseBook=null,priorPurchaseProof=null
 try{
  sql('postgres',`CREATE DATABASE ${db} OWNER archava_owner`);created=true
  ops(previous,db,['migrate','--runtime-role','archava_runtime'])
  ops(previous,db,['bootstrap-owner','--tenant',tenant,'--principal','native-owner','--username',username])
  oldServer=await start(previous,db,'prior-native-upgrade-before.log')
  const originalSession=await login(oldServer.origin)
  // The immutable prior package's bootstrap predates explicit party grants.
  // Declare this administrator fixture setup; never grant them by upgrading.
  let explicitPriorPartyProvisioning=false
  if(priorInvoiceSources){
   const permissions=JSON.parse(sql(db,`SET archava.tenant_id='${tenant}';SELECT to_json(permissions) FROM archava_organizations.memberships WHERE tenant_id='${tenant}' AND principal_id='native-owner'`))
   if(!['parties.read','parties.write'].every(p=>permissions.includes(p))){
    sql(db,`SET archava.tenant_id='${tenant}';UPDATE archava_organizations.memberships SET permissions=ARRAY(SELECT DISTINCT unnest(permissions||ARRAY['parties.read','parties.write'])) WHERE tenant_id='${tenant}' AND principal_id='native-owner'`)
    explicitPriorPartyProvisioning=true
   }
  }

  const org=async(type,data)=>{const r=await call(oldServer.origin,originalSession,'/erp/organizations/commands',{tenant,mutation:{requestId:randomUUID(),type,data}});assert.equal(r.status,200,JSON.stringify(r.body));return r.body.receipt}
  const source=await org('create_organization',{name:'Existing business',legalName:'Existing Business Official'})
  const company=await org('company.create_from_organization',{code:'EXISTING',organizationId:source.organizationId,expectedSourceRevision:1,reason:'Reviewed company on previous native distribution'})
  const command={tenant,requestId:randomUUID(),command:{operation:'product.create_for_company',companyId:company.companyId,expectedCompanyRevision:1,sku:'PRE-BOOK',title:'Keep the original product and receipt',unit:'unit',trackStock:false,price:15000}}
  // A previous distribution that already has the book guard refuses a USD book once the company owns a product,
  // so there the book is created first and the product (then priced in the book currency) after it.
  const bookFirst=priorAccounting&&bookGuard(oldManifest)
  const createProduct=async()=>{const r=await call(oldServer.origin,originalSession,'/erp/commerce/commands',command);assert.equal(r.status,200,JSON.stringify(r.body));return r}
  let written=bookFirst?null:await createProduct()
  if(priorAccounting){
   const financial=async command=>{const r=await call(oldServer.origin,originalSession,'/erp/accounting/commands',{tenant,requestId:randomUUID(),command});assert.equal(r.status,200,JSON.stringify(r.body));return r.body.receipt}
   priorBook=await financial({type:'book.create',companyId:company.companyId,expectedCompanyRevision:1,code:'EXISTING-BOOK',name:'Reviewed book on the prior native distribution',currency:'USD',sourceAuthority:'native',independentPosting:false,reason:'Explicit synthetic prior book; preserve its posted balances on upgrade'})
   if(bookFirst)written=await createProduct()
   const bank=await financial({type:'account.create',bookId:priorBook.bookId,code:'BANK',name:'Prior bank',class:'asset',cashRole:'cash',reason:'Reviewed prior account'})
   const equity=await financial({type:'account.create',bookId:priorBook.bookId,code:'EQUITY',name:'Prior equity',class:'equity',cashRole:'financing',reason:'Reviewed prior account'})
   await financial({type:'period.open',bookId:priorBook.bookId,code:'2026-11',startDate:'2026-11-01',endDate:'2026-11-30',reason:'Prior financial period'})
   const journal=await financial({type:'journal.draft',bookId:priorBook.bookId,code:'BEFORE-UPGRADE',date:'2026-11-07',sourceRef:'Explicit prior source',memo:'Preserve the actual old posted financial history',lines:[{accountId:bank.resourceId,expectedAccountRevision:1,debitMinor:'10000',creditMinor:'0',memo:'Prior cash'},{accountId:equity.resourceId,expectedAccountRevision:1,debitMinor:'0',creditMinor:'10000',memo:'Prior equity'}],reason:'Reviewed prior USD 100.00 balance'})
   await financial({type:'journal.post',id:journal.resourceId,expectedRevision:1,reason:'Post under the explicitly configured prior single-operator policy'})
  }
  if(priorInvoiceSources){
   const financial=async command=>{const r=await call(oldServer.origin,originalSession,'/erp/accounting/commands',{tenant,requestId:randomUUID(),command});assert.equal(r.status,200,JSON.stringify(r.body));return r.body.receipt}
   const trade=async command=>{const r=await call(oldServer.origin,originalSession,'/erp/commerce/commands',{tenant,requestId:randomUUID(),command});assert.equal(r.status,200,JSON.stringify(r.body));return r.body.receipt}
   const bill=async mutation=>{const r=await call(oldServer.origin,originalSession,'/erp/billing/commands',{tenant,mutation:{commandId:randomUUID(),...mutation}});assert.equal(r.status,200,JSON.stringify(r.body));return r.body}
   const sourceOrg=await org('create_organization',{name:'Prior invoice company',legalName:'Prior Invoice Company Official'})
   const c=await org('company.create_from_organization',{code:'PRIOR-SOURCE',organizationId:sourceOrg.organizationId,expectedSourceRevision:1,reason:'Preserve actual issued invoice source history across code upgrades'})
   priorSourceBook=await financial({type:'book.create',companyId:c.companyId,expectedCompanyRevision:1,code:'PRIOR-IDR',name:'Prior native invoice-source book',currency:'IDR',sourceAuthority:'native',independentPosting:false,reason:'Reviewed synthetic whole-Rupiah policy'})
   const accounts={};for(const[role,code,cls,cash]of [['receivable','AR','asset','operating'],['revenue','REVENUE','revenue','operating'],['payment_clearing','CLEARING','asset','none']])accounts[role]=await financial({type:'account.create',bookId:priorSourceBook.bookId,code,name:code,class:cls,cashRole:cash,reason:'Prior invoice account'})
   await financial({type:'period.open',bookId:priorSourceBook.bookId,code:'PRIOR-SOURCE',startDate:'2000-01-01',endDate:'2100-12-31',reason:'Owned prior source date range'})
   await financial({type:'invoice_policy.set',bookId:priorSourceBook.bookId,expectedRevision:0,currencyCatalogue:'SIX-2026-09-17',sourceUnit:'whole_idr',mappings:Object.entries(accounts).map(([role,a])=>({role,accountId:a.resourceId,expectedAccountRevision:1})),reason:'Explicit prior whole-Rupiah unit contract; no bank assertion'})
   const buyer=await org('create_organization',{name:'Prior invoice buyer',legalName:'Prior Invoice Buyer Official'})
   const party=await org('party.create_from_organization',{organizationId:buyer.organizationId,expectedSourceRevision:1,customer:true,supplier:false})
   const product=await trade({operation:'product.create_for_company',companyId:c.companyId,expectedCompanyRevision:1,sku:'PRIOR-SOURCE',title:'Prior invoice service',unit:'unit',trackStock:false,price:15000})
   const order=await trade({operation:'order.create_for_company',companyId:c.companyId,expectedCompanyRevision:1,partyId:party.partyId,expectedPartyRevision:1,lines:[{productId:product.id,quantity:1}]})
   await trade({operation:'order.reserve',id:order.id,expectedRevision:1,holdSeconds:300});await trade({operation:'order.fulfill',id:order.id,expectedRevision:2})
   const invoice=await bill({type:'create_invoice_from_order',invoiceCode:'PRIOR-SOURCE',orderId:order.id,expectedOrderRevision:3})
   const issue=await bill({type:'issue_invoice',invoiceId:invoice.invoiceId,expectedRevision:1,reason:'Issue using the actual prior native binary'})
   const journal=await financial({type:'invoice_source.draft',sourceId:issue.commandId,code:'PRIOR-ISSUE',accounts:[accounts.receivable,accounts.revenue].map(a=>({accountId:a.resourceId,expectedAccountRevision:1})),reason:'Review actual prior captured invoice source'})
   await financial({type:'journal.post',id:journal.resourceId,expectedRevision:1,reason:'Post the reviewed prior source journal'})
  }
  const purchaseProof=database=>JSON.parse(sql(database,`SELECT jsonb_build_object('sources',(SELECT jsonb_agg(to_jsonb(r) ORDER BY tenant_id,id) FROM archava_accounting.purchase_sources r),'origins',(SELECT jsonb_agg(to_jsonb(r) ORDER BY tenant_id,id) FROM archava_accounting.purchase_origins r),'layers',(SELECT jsonb_agg(CASE WHEN ${priorStockSources?'true':'false'} THEN to_jsonb(r) ELSE to_jsonb(r)-'return_cost_id' END ORDER BY tenant_id,id) FROM archava_commerce.inventory_layers r),'uses',(SELECT jsonb_agg(to_jsonb(r) ORDER BY tenant_id,id) FROM archava_commerce.inventory_uses r),'returns',(SELECT jsonb_agg(to_jsonb(r) ORDER BY tenant_id,id) FROM archava_commerce.inventory_return_costs r),'stock',(SELECT jsonb_agg(to_jsonb(r) ORDER BY tenant_id,product_id) FROM archava_commerce.stock_balances r));`))
  // A later migration may add columns to a prior purchase source row. They must arrive empty (null or zero); every prior column must be unchanged.
  const upgradedPurchaseProof=database=>{const after=purchaseProof(database);for(const [i,row] of (after.sources||[]).entries()){const prior=priorPurchaseProof.sources?.[i]||{};for(const key of Object.keys(row))if(!Object.hasOwn(prior,key)){assert.ok(row[key]===null||row[key]===0,`Upgrade filled new purchase source column ${key}`);delete row[key]}}return after}
  if(priorPurchaseSources){
   const mutate=async(domain,command)=>{const requestId=randomUUID(),r=await call(oldServer.origin,originalSession,`/erp/${domain}/commands`,{tenant,requestId,command});assert.equal(r.status,200,JSON.stringify(r.body));return{...r.body.receipt,requestId:r.body.requestId??r.body.receipt.requestId}}
   const financial=command=>mutate('accounting',command)
   const sourceOrg=await org('create_organization',{name:'Prior acquired stock company',legalName:'Prior Acquired Stock Official'})
   const c=await org('company.create_from_organization',{code:'PRIOR-STOCK',organizationId:sourceOrg.organizationId,expectedSourceRevision:1,reason:'Actual prior acquired costs before source migration'})
   priorPurchaseBook=await financial({type:'book.create',companyId:c.companyId,expectedCompanyRevision:1,code:'PRIOR-STOCK',name:'Prior actual purchase source book',currency:'IDR',sourceAuthority:'native',independentPosting:false,reason:'Owned prior acquired-cost profile'})
   const accounts={};for(const[role,cls]of [['inventory','asset'],['goods_received_not_invoiced','liability'],['payable','liability'],['landed_fee_accrual','liability']])accounts[role]=await financial({type:'account.create',bookId:priorPurchaseBook.bookId,code:role.toUpperCase(),name:role,class:cls,cashRole:'operating',reason:'Prior acquired-cost account'})
   await financial({type:'period.open',bookId:priorPurchaseBook.bookId,code:'PRIOR-STOCK',startDate:'2000-01-01',endDate:'2100-12-31',reason:'Owned prior acquired-cost source range'})
   await financial({type:'purchase_policy.set',bookId:priorPurchaseBook.bookId,expectedRevision:0,currencyCatalogue:'SIX-2026-09-17',sourceUnit:'whole_idr',mappings:Object.entries(accounts).map(([role,a])=>({role,accountId:a.resourceId,expectedAccountRevision:1})),reason:'Actual prior native purchase source policy'})
   const supplierOrg=await org('create_organization',{name:'Prior source supplier',legalName:'Prior Source Supplier Official'}),supplierParty=await org('party.create_from_organization',{organizationId:supplierOrg.organizationId,expectedSourceRevision:1,customer:false,supplier:true})
   const supplier=await mutate('purchasing',{operation:'supplier.create-from-party',code:'PRIOR-SUPPLIER',partyId:supplierParty.partyId,expectedPartyRevision:1})
   const product=await mutate('commerce',{operation:'product.create_for_company',companyId:c.companyId,expectedCompanyRevision:1,sku:'PRIOR-ACQUIRED',title:'Actual original acquisition layers before upgrade',unit:'unit',trackStock:true,price:15000})
   const offer=await mutate('purchasing',{operation:'offer.create',supplierId:supplier.id,productId:product.id,unitCost:7000}),po=await mutate('purchasing',{operation:'order.create-for-company',companyId:c.companyId,expectedCompanyRevision:1,supplierId:supplier.id,lines:[{offerId:offer.id,quantity:3}]})
   await mutate('purchasing',{operation:'order.confirm',id:po.id,expectedRevision:1,reason:'Capture the actual prior source policy'})
   const line=sql(db,`SELECT id FROM archava_purchasing.lines WHERE order_id='${po.id}'`),received=await mutate('purchasing',{operation:'order.receive',id:po.id,expectedRevision:2,reason:'Prior accepted and rejected goods',landedFeeMinor:101,allocationBasis:'value',lines:[{lineId:line,accepted:2,rejected:1}]})
   const document=await mutate('purchasing',{operation:'document.capture',orderId:po.id,expectedOrderRevision:3,supplierId:supplier.id,vendorReference:'PRIOR-SUPPLIER-BILL',documentDate:'2026-11-08',currency:'IDR',declaredTotal:14000,lines:[{lineId:line,quantity:2,unitCost:7000}]}),matched=await mutate('purchasing',{operation:'document.match',id:document.id,expectedRevision:1,expectedOrderRevision:3,reason:'Match actual accepted goods under the old native binary'})
   for(const[source,code,roles]of [[received,'PRIOR-RECEIPT',['inventory','goods_received_not_invoiced','landed_fee_accrual']],[matched,'PRIOR-BILL',['goods_received_not_invoiced','payable']]]){
    const journal=await financial({type:'purchase_source.draft',sourceId:source.requestId,code,accounts:roles.map(role=>({accountId:accounts[role].resourceId,expectedAccountRevision:1})),reason:'Actual prior source journal review'});await financial({type:'journal.post',id:journal.resourceId,expectedRevision:1,reason:'Actual prior source posting'})
   }
   const buyerOrg=await org('create_organization',{name:'Prior stock buyer',legalName:'Prior Stock Buyer Official'}),buyer=await org('party.create_from_organization',{organizationId:buyerOrg.organizationId,expectedSourceRevision:1,customer:true,supplier:false})
   const sale=await mutate('commerce',{operation:'order.create_for_company',companyId:c.companyId,expectedCompanyRevision:1,partyId:buyer.partyId,expectedPartyRevision:1,lines:[{productId:product.id,quantity:1}]});await mutate('commerce',{operation:'order.reserve',id:sale.id,expectedRevision:1,holdSeconds:300});await mutate('commerce',{operation:'order.fulfill',id:sale.id,expectedRevision:2})
   const saleLine=sql(db,`SELECT id FROM archava_commerce.order_lines WHERE order_id='${sale.id}'`);await mutate('commerce',{operation:'order.return',id:sale.id,expectedRevision:3,lineId:saleLine,quantity:1,disposition:'restock',reason:'Actual legacy return before immutable return layer provenance'})
   priorPurchaseProof=purchaseProof(db);assert.equal(priorPurchaseProof.sources.length,2);assert.equal(priorPurchaseProof.layers.length,2)
  }
  const before=proof(db);assert.equal(before.products.length,priorPurchaseSources?3:priorInvoiceSources?2:1)
  const priorSourceProof=priorInvoiceSources?JSON.parse(sql(db,"SELECT jsonb_build_object('sources',(SELECT count(*) FROM archava_accounting.invoice_sources),'policies',(SELECT count(*) FROM archava_accounting.invoice_policies),'links',(SELECT count(*) FROM archava_accounting.journal_sources),'invoices',(SELECT jsonb_agg(to_jsonb(r) ORDER BY tenant_id,id) FROM archava_billing.invoices r),'commands',(SELECT jsonb_agg(to_jsonb(r) ORDER BY tenant_id,request_id) FROM archava_billing.commands r));")):null
  if(priorInvoiceSources)assert.deepEqual([priorSourceProof.sources,priorSourceProof.policies,priorSourceProof.links],[1,1,1])

  assert.equal(sql(db,`SELECT to_regclass('archava_accounting.${priorAccounting?'invoice_sources':'books'}') IS NULL`),priorInvoiceSources?'f':'t')
  const capture=ops(previous,db,['backup','--port',String(facts.port),'--db',db,'--user','postgres','--out',backup]);assert.equal(capture.backup.credentialValuesPrinted,false)
  ops(previous,db,['drain-writer']);await terminate(oldServer.child);oldServer=null
  const migrated=ops(current,db,['migrate','--runtime-role','archava_runtime'])
  assert.equal(migrated.installation.migration.applied,expectedAdded,'Only the verified declared migration suffix is added to this previous candidate')
  assert.equal(ops(current,db,['migrate','--runtime-role','archava_runtime']).installation.migration.applied,0)
  if(newDisclosure&&!oldDisclosure){
   const port=await freePort(),env={...productionEnvironment,DATABASE_URL:`postgres://archava_runtime:archava@127.0.0.1:${facts.port}/${db}`,ARCHAVA_BIND_ADDR:`127.0.0.1:${port}`,ARCHAVA_SITE_ROOT:join(previous,'site'),ARCHAVA_WRITER_TAKEOVER:'false',GEMINI_AGENT_MODEL:'',GEMINI_AGENT_API_KEY:'',GEMINI_API_KEY:''}
   const child=startServer(join(previous,'bin/archava-server'),env,'prior-native-disclosure-downgrade-refused.log')
   let refusal='';for(const stream of [child.stdout,child.stderr])stream.on('data',chunk=>{refusal+=chunk})
   try{await until(()=>child.exitCode!==null,'old binary disclosure-profile refusal',20000);assert.notEqual(child.exitCode,0,'A pre-disclosure native binary must not serve the upgraded Owner profile');assert.match(refusal,/MigrationHistory|migration history must match/,'The previous executable must refuse its migration profile, not an unrelated startup problem');if(expectedAdded===1)assert.match(refusal,/own/i,'The single Owner migration must cause the downgrade refusal')}
   finally{await terminate(child)}
   pass('OPS-disclosure-downgrade-refused','The actual previous native executable exits on the upgraded migration profile, preventing a rollback from silently ignoring provider-sharing policies')
  }
  assert.deepEqual(proof(db),before)
  if(newPayables&&!oldPayables){
   assert.equal(sql(db,'SELECT count(*) FROM archava_accounting.payable_settlements'),'0','Upgrade must not invent supplier payment history')
   assert.equal(sql(db,"SELECT count(*) FROM archava_accounting.memberships WHERE permissions&&ARRAY['payables.read','payables.write']::text[]"),'0','Upgrade must not silently grant existing users supplier settlement authority')
  }
  if(newClearing&&!oldClearing){
   assert.equal(sql(db,'SELECT count(*) FROM archava_accounting.clearing_settlements'),'0','Upgrade must not invent gateway payouts')
   assert.equal(sql(db,"SELECT count(*) FROM archava_accounting.memberships WHERE permissions&&ARRAY['clearing.read','clearing.write']::text[]"),'0','Upgrade must not grant existing users gateway settlement authority')
  }
  if(newTerms&&!oldTerms){
   assert.equal(sql(db,'SELECT count(*) FROM archava_accounting.document_terms'),'0','Upgrade must not invent historical due agreements')
   assert.equal(sql(db,"SELECT count(*) FROM archava_accounting.memberships WHERE permissions&&ARRAY['terms.read','terms.write']::text[]"),'0','Upgrade must not grant existing users due-term authority')
  }
  if(taxCodes(newManifest)&&!taxCodes(oldManifest)){
   assert.equal(sql(db,'SELECT count(*) FROM archava_accounting.tax_codes'),'0','Upgrade must not invent tax codes or rates')
   assert.equal(sql(db,"SELECT count(*) FROM archava_accounting.memberships WHERE permissions&&ARRAY['tax.read','tax.manage']::text[]"),'0','Upgrade must not grant existing users tax code authority')
  }
  if(invoiceTax(newManifest)&&!invoiceTax(oldManifest)){
   assert.equal(sql(db,'SELECT count(*) FROM archava_billing.invoices WHERE tax_minor<>0 OR correction_tax_minor<>0 OR tax_code_id IS NOT NULL'),'0','Upgrade must not tax an existing invoice')
   assert.equal(sql(db,'SELECT count(*) FROM archava_billing.invoice_corrections WHERE tax_minor<>0'),'0','Upgrade must not split an existing correction into tax')
  }
  if(sourceTax(newManifest)&&!sourceTax(oldManifest)&&priorInvoiceSources){
   assert.equal(sql(db,'SELECT count(*) FROM archava_accounting.invoice_sources WHERE tax_minor<>0 OR tax_account_id IS NOT NULL OR tax_account_revision IS NOT NULL'),'0','Upgrade must not put tax on an existing invoice source')
  }
  if(purchaseSourceTax(newManifest)&&!purchaseSourceTax(oldManifest)&&priorPurchaseSources){
   assert.equal(sql(db,'SELECT count(*) FROM archava_accounting.purchase_sources WHERE tax_minor<>0 OR tax_account_id IS NOT NULL OR tax_account_revision IS NOT NULL'),'0','Upgrade must not put tax on an existing purchase source')
  }
  if(documentTax(newManifest)&&!documentTax(oldManifest)){
   assert.equal(sql(db,'SELECT count(*) FROM archava_purchasing.supplier_documents WHERE tax_minor<>0 OR tax_computed_minor<>0 OR tax_code_id IS NOT NULL OR tax_code IS NOT NULL OR tax_code_version IS NOT NULL OR tax_rate_numerator IS NOT NULL OR tax_rate_denominator IS NOT NULL OR tax_base_numerator IS NOT NULL OR tax_base_denominator IS NOT NULL'),'0','Upgrade must not put tax on an existing supplier document')
   assert.equal(sql(db,"SELECT count(*) FROM archava_purchasing.supplier_document_versions WHERE snapshot ? 'tax_minor' OR snapshot ? 'tax_code_id'"),'0','Upgrade must not rewrite a stored supplier document version')
  }
  if(taxRegistration(newManifest)&&!taxRegistration(oldManifest)){
   assert.equal(sql(db,'SELECT count(*) FROM archava_organizations.companies WHERE vat_registered'),'0','Upgrade must not state a tax registration for an existing company')
   assert.equal(sql(db,'SELECT count(*) FROM archava_organizations.parties WHERE tax_id IS NOT NULL'),'0','Upgrade must not copy a tax id onto an existing party')
   assert.equal(sql(db,"SELECT count(*) FROM archava_organizations.party_audit WHERE snapshot ? 'tax_id'"),'0','Upgrade must not rewrite a stored party version')
  }
  if(invoiceIdentifiers(newManifest)&&!invoiceIdentifiers(oldManifest)){
   assert.equal(sql(db,'SELECT count(*) FROM archava_billing.invoices WHERE seller_tax_id IS NOT NULL OR buyer_tax_id IS NOT NULL OR tax_reference IS NOT NULL OR tax_transaction_code IS NOT NULL OR tax_reference_version<>0'),'0','Upgrade must not put tax ids or a tax invoice reference on an existing invoice')
  }
  if(salesTax(newManifest)&&!salesTax(oldManifest)){
   assert.equal(sql(db,'SELECT count(*) FROM archava_commerce.orders WHERE tax_code IS NOT NULL OR price_includes_tax IS NOT NULL'),'0','Upgrade must not put a tax code on an existing sales order')
  }
  if(addedBank){
   assert.equal(sql(db,`SELECT ${bankTables.map(table=>`(SELECT count(*) FROM archava_accounting.${table})`).join('+')}`),'0','Upgrade must not invent bank accounts, statements, matches, opening policies or reviews')
   assert.equal(sql(db,"SELECT count(*) FROM archava_accounting.memberships WHERE permissions&&ARRAY['bank.read','bank.configure','bank.import','bank.reconcile']::text[]"),'0','New bank authority is never added to existing memberships')
  }
  if(!priorAccounting)assert.equal(sql(db,"SELECT (SELECT count(*) FROM archava_accounting.books)+(SELECT count(*) FROM archava_accounting.journals)+(SELECT count(*) FROM archava_accounting.memberships)"),'0')
  if(!priorInvoiceSources&&newManifest.ledgers.find(l=>l.namespace==='archava_accounting')?.migrations.some(m=>m.id==='002_invoice_source_posting'))assert.equal(sql(db,"SELECT (SELECT count(*) FROM archava_accounting.invoice_policies)+(SELECT count(*) FROM archava_accounting.invoice_sources)+(SELECT count(*) FROM archava_accounting.journal_sources)"),'0','Upgrade must not invent source policies or source postings')
  if(!priorPurchaseSources&&newManifest.ledgers.find(l=>l.namespace==='archava_accounting')?.migrations.some(m=>m.id==='004_purchase_source_posting'))assert.equal(sql(db,"SELECT (SELECT count(*) FROM archava_accounting.purchase_policies)+(SELECT count(*) FROM archava_accounting.purchase_origins)+(SELECT count(*) FROM archava_accounting.purchase_sources)+(SELECT count(*) FROM archava_accounting.purchase_journal_sources)"),'0','Upgrade must not invent purchase setup, origins, sources or postings')
  if(priorPurchaseSources){
   assert.deepEqual(upgradedPurchaseProof(db),priorPurchaseProof)
   assert.equal(sql(db,"SELECT count(*) FROM archava_accounting.stock_layer_origins"),priorStockSources?'2':'1',priorStockSources?'Both already-known acquired and returned layer origins survive the upgrade':'Only the exact acquired layer gets its reviewed prior inventory account; legacy return lineage stays unguessed')
   assert.equal(sql(db,"SELECT (SELECT count(*) FROM archava_accounting.stock_policies)+(SELECT count(*) FROM archava_accounting.stock_sources)+(SELECT count(*) FROM archava_accounting.stock_journal_sources)"),'0','Upgrade never invents historical COGS recognition or stock policy')
  }
  newServer=await start(current,db,'prior-native-upgrade-after.log')
  const currentSession=await login(newServer.origin)
  const replay=await call(newServer.origin,currentSession,'/erp/commerce/commands',command)
  assert.equal(replay.status,200,JSON.stringify(replay.body));assert.deepEqual(replay.body.receipt,written.body.receipt,'Replay returns the original durable receipt')
  assert.deepEqual(proof(db),before)
  const denied=await call(newServer.origin,currentSession,`/erp/accounting/workspace?tenant=${tenant}&companyId=${company.companyId}`)
  if(priorAccounting){
   assert.equal(denied.status,200);assert.equal(denied.body.configuredBook.id,priorBook.bookId)
   if(addedBank){
    assert.equal(denied.body.bank_accounts,undefined)
    const bankRead=await call(newServer.origin,currentSession,`/erp/accounting/workspace?tenant=${tenant}&companyId=${company.companyId}&collection=bank_accounts`)
    assert.equal(bankRead.status,403,'A prior owner with books.read does not acquire bank.read during upgrade')
   }
   const r=await call(newServer.origin,currentSession,`/erp/accounting/reports?tenant=${tenant}&bookId=${priorBook.bookId}&from=2026-11-01&to=2026-11-30`)
   assert.equal(r.status,200);assert.equal(r.body.balanceSheet.assetsMinor,'10000');assert.equal(r.body.trialBalance.balanced,true)
  }else assert.equal(denied.status,403,'Upgrading must not grant an existing owner new financial authority')
  const verifyPriorSources=async(database,server,session)=>{
   if(!priorInvoiceSources)return
   const after=JSON.parse(sql(database,"SELECT jsonb_build_object('sources',(SELECT count(*) FROM archava_accounting.invoice_sources),'policies',(SELECT count(*) FROM archava_accounting.invoice_policies),'links',(SELECT count(*) FROM archava_accounting.journal_sources),'invoices',(SELECT jsonb_agg(to_jsonb(r) ORDER BY tenant_id,id) FROM archava_billing.invoices r),'commands',(SELECT jsonb_agg(to_jsonb(r) ORDER BY tenant_id,request_id) FROM archava_billing.commands r));"))
   // A later migration may add columns to a prior invoice row. They must arrive empty (null or zero); every prior column must be unchanged.
   for(const [i,row] of (after.invoices||[]).entries()){const prior=priorSourceProof.invoices?.[i]||{};for(const key of Object.keys(row))if(!Object.hasOwn(prior,key)){assert.ok(row[key]===null||row[key]===0,`Upgrade filled new invoice column ${key}`);delete row[key]}}
   assert.deepEqual(after,priorSourceProof)
   const r=await call(server.origin,session,`/erp/accounting/reports?tenant=${tenant}&bookId=${priorSourceBook.bookId}&from=2000-01-01&to=2100-12-31`)
   assert.equal(r.status,200);assert.equal(r.body.balanceSheet.assetsMinor,'1500000');assert.equal(r.body.trialBalance.balanced,true);assert.equal(r.body.sourceCoverage.postedCount,1);assert.equal(r.body.sourceCoverage.unpostedCount,0)
  }
  const verifyPriorPurchases=async(database,server,session)=>{
   if(!priorPurchaseSources)return
   assert.deepEqual(upgradedPurchaseProof(database),priorPurchaseProof)
   const r=await call(server.origin,session,`/erp/accounting/reports?tenant=${tenant}&bookId=${priorPurchaseBook.bookId}&from=2000-01-01&to=2100-12-31`)
   assert.equal(r.status,200,JSON.stringify(r.body));assert.equal(r.body.balanceSheet.assetsMinor,'1410100');assert.equal(r.body.profitAndLoss.costOfGoodsSoldMinor,'0');assert.equal(r.body.trialBalance.balanced,true);assert.equal(r.body.sourceCoverage.postedCount,2);assert.equal(r.body.sourceCoverage.unpostedCount,0)
  }
  await verifyPriorPurchases(db,newServer,currentSession)
  await verifyPriorSources(db,newServer,currentSession)
  pass('OPS-prior-native-financial-upgrade','The actual previous native distribution creates the business; current verified migration preserves the seeded source rows/receipt and existing financial authority boundary, and replays the old command without a duplicate')

  await terminate(newServer.child);newServer=null
  sql('postgres',`CREATE DATABASE ${rollback} OWNER archava_owner`);rollbackCreated=true
  ops(previous,rollback,['restore','--manifest',join(backup,'archava-backup.manifest.json'),'--port',String(facts.port),'--db',rollback,'--user','postgres'])
  assert.deepEqual(proof(rollback),before)
  assert.equal(sql(rollback,`SELECT to_regclass('archava_accounting.${priorAccounting?'invoice_sources':'books'}') IS NULL`),priorInvoiceSources?'f':'t')
  rollbackServer=await start(previous,rollback,'prior-native-rollback.log')
  const recovered=await login(rollbackServer.origin)
  const view=await call(rollbackServer.origin,recovered,`/erp/commerce/workspace?tenant=${tenant}&companyId=${company.companyId}`)
  assert.equal(view.status,200);assert.equal(view.body.products.length,1);assert.equal(view.body.products[0].price,15000)
  if(priorAccounting){const r=await call(rollbackServer.origin,recovered,`/erp/accounting/reports?tenant=${tenant}&bookId=${priorBook.bookId}&from=2026-11-01&to=2026-11-30`);assert.equal(r.status,200);assert.equal(r.body.balanceSheet.assetsMinor,'10000')}
  await verifyPriorSources(rollback,rollbackServer,recovered)
  await verifyPriorPurchases(rollback,rollbackServer,recovered)
  pass('OPS-prior-native-isolated-rollback','The encrypted previous-profile backup restores into a separate owned database, retains the original records, and serves/logs in using the previous native binary; no reverse SQL is applied to the upgraded database')
  sql('postgres',`CREATE DATABASE ${fresh} OWNER archava_owner`);freshCreated=true
  ops(current,fresh,['migrate','--runtime-role','archava_runtime'])
  ops(current,fresh,['bootstrap-owner','--tenant',tenant,'--principal','native-owner','--username',username])
  freshServer=await start(current,fresh,'fresh-native-owner-business-identity.log')
  const freshSession=await login(freshServer.origin)
  const freshOrg=await call(freshServer.origin,freshSession,'/erp/organizations/commands',{tenant,mutation:{requestId:randomUUID(),type:'create_organization',data:{name:'Fresh native customer',legalName:'Fresh Native Customer Official'}}})
  assert.equal(freshOrg.status,200,JSON.stringify(freshOrg.body))
  const freshParty=await call(freshServer.origin,freshSession,'/erp/organizations/commands',{tenant,mutation:{requestId:randomUUID(),type:'party.create_from_organization',data:{organizationId:freshOrg.body.receipt.organizationId,expectedSourceRevision:1,customer:true,supplier:true}}})
  assert.equal(freshParty.status,200,JSON.stringify(freshParty.body));assert.match(freshParty.body.receipt.partyId,/^[0-9a-f-]{36}$/)
  pass('OPS-fresh-native-owner-party-provisioning','A freshly migrated current distribution bootstraps an owner, logs in through its real native server and creates a business party with customer/supplier roles without additional administrator grants')
  let freshBankVerified=false
  if(newBank){
   const freshCommand=async command=>{const r=await call(freshServer.origin,freshSession,'/erp/accounting/commands',{tenant,requestId:randomUUID(),command});assert.equal(r.status,200,JSON.stringify(r));return r.body.receipt}
   const c=await call(freshServer.origin,freshSession,'/erp/organizations/commands',{tenant,mutation:{requestId:randomUUID(),type:'company.create_from_organization',data:{organizationId:freshOrg.body.receipt.organizationId,expectedSourceRevision:1,code:'FRESH-BANK',reason:'Explicit owned fresh native bank acceptance company'}}});assert.equal(c.status,200,JSON.stringify(c))
   const b=await freshCommand({type:'book.create',companyId:c.body.receipt.companyId,expectedCompanyRevision:1,code:'FRESH-BANK',name:'Fresh installed native bank',currency:'USD',sourceAuthority:'native',independentPosting:false,reason:'Explicit synthetic single-operator book'})
   const a=await freshCommand({type:'account.create',bookId:b.bookId,code:'BANK',name:'Fresh native bank cash',class:'asset',cashRole:'cash',reason:'Reviewed actual cash classification'})
   const p=await freshCommand({type:'period.open',bookId:b.bookId,code:'2026-11',startDate:'2026-11-01',endDate:'2026-11-30',reason:'Complete native-installed empty bank period'})
   const bank=await freshCommand({type:'bank_account.create',bookId:b.bookId,code:'LOCAL',name:'Explicit fresh manual bank',account:{accountId:a.resourceId,expectedAccountRevision:1},reason:'Exercise actual installed non-owner runtime grants'})
   const policy=await freshCommand({type:'bank_policy.set',bankAccountId:bank.resourceId,expectedRevision:0,fromDate:'2026-11-01',independentReview:false,reason:'Explicit synthetic one-person bank review; not an inferred policy'})
   await freshCommand({type:'bank_statement.import',bankAccountId:bank.resourceId,account:{accountId:a.resourceId,expectedAccountRevision:1},statement:{reference:'FRESH-NOV',fromDate:'2026-11-01',toDate:'2026-11-30',currency:'USD',openingMinor:'0',closingMinor:'0',rows:[]},reason:'Explicit full empty statement, not an automatically invented opening'})
   const current=await call(freshServer.origin,freshSession,`/erp/accounting/bank-reconciliation?tenant=${tenant}&bankPolicyId=${policy.resourceId}&periodId=${p.resourceId}`);assert.equal(current.status,200);assert.equal(current.body.check.ready,true);assert.equal(current.body.canConfirm,true)
   await freshCommand({type:'bank_reconciliation.confirm',bankPolicyId:policy.resourceId,expectedPolicyRevision:1,periodId:p.resourceId,expectedPeriodRevision:1,expectedCheckHash:current.body.check.evidenceSha256,reason:'Human explicitly confirms the native installed complete bank controls'})
   await freshCommand({type:'period.close',id:p.resourceId,expectedRevision:1,reason:'Full empty statement and current explicit review agree'})
   const closed=await call(freshServer.origin,freshSession,`/erp/accounting/bank-reconciliation?tenant=${tenant}&bankPolicyId=${policy.resourceId}&periodId=${p.resourceId}`);assert.equal(closed.status,200);assert.equal(closed.body.periodState,'closed');assert.equal(closed.body.reviewed,true)
   freshBankVerified=true
   pass('OPS-fresh-native-bank-grants','Actual package migration and owner bootstrap, without administrator grant fixes, serve non-owner bank registration, explicit opening basis, immutable full statement, SHA-bound review and period close')
  }
  const freshPayablesVerified=newPayables?await verifyNativePayables({server:freshServer,session:freshSession,tenant,call,sql:input=>sql(fresh,input),terms:newTerms,pass}):null
  let freshClearingAccess=null
  if(newClearing){
   const permissions=JSON.parse(sql(fresh,`SET archava.tenant_id='${tenant}';SELECT to_json(permissions) FROM archava_accounting.memberships WHERE tenant_id='${tenant}' AND principal_id='native-owner'`))
   assert.ok(['clearing.read','clearing.write'].every(p=>permissions.includes(p)))
   const companyId=sql(fresh,`SET archava.tenant_id='${tenant}';SELECT company_id FROM archava_accounting.books WHERE tenant_id='${tenant}' AND code='FRESH-PAYABLE'`)
   for(const collection of ['clearing_sources','clearing_settlements']){const r=await call(freshServer.origin,freshSession,`/erp/accounting/workspace?tenant=${tenant}&companyId=${companyId}&collection=${collection}`);assert.equal(r.status,200,JSON.stringify(r));assert.ok(Array.isArray(r.body[collection]))}
   freshClearingAccess={installedOwnerPermissions:true,actualRuntimeReads:2,extraAdministratorGrants:false,positiveSettlementFlow:false,scope:'Fresh installed read/permission boundary; positive verified-source settlements are covered separately by the owned browser/provider fixture'}
   pass('OPS-fresh-native-clearing-read-grants',freshClearingAccess)
  }
  return{previousDistribution:previous,currentDistribution:current,applied:migrated.installation.migration.applied,idempotentApply:true,originalRowsAndReceiptsMatch:true,freshOwnerBusinessIdentityVerified:true,freshOwnerExtraAdministratorGrants:false,freshBankVerified,freshPayablesVerified,freshClearingAccess,priorPostedBookPreserved:priorAccounting,priorInvoiceSourcesPreserved:priorInvoiceSources,priorPurchaseSourcesPreserved:priorPurchaseSources,priorAcquiredAndReturnedCostsPreserved:priorPurchaseSources,explicitPriorPartyProvisioning,priorFinancialTablesVerified:priorAccounting?priorFinancialTables.length:0,oldCommandReplayed:true,implicitFinancialBooks:0,implicitFinancialGrants:0,implicitBankEvidence:0,implicitBankGrants:0,implicitPayableHistory:0,implicitPayableGrants:0,implicitClearingHistory:0,implicitClearingGrants:0,implicitDueTerms:0,implicitTermsGrants:0,freshTermsVerified:freshPayablesVerified?.dueTerms??null,implicitSourcePolicies:0,implicitSourcePostings:0,encryptedPriorBackup:true,isolatedPriorBinaryRollback:true,privacyDowngradeRefused:newDisclosure&&!oldDisclosure,scope:'Owned local synthetic databases; production-host RTO/RPO and actual-user acceptance remain separate; current multi-ledger refusal does not isolate the Owner migration as its sole cause'}
 }finally{
  for(const server of [oldServer,newServer,rollbackServer,freshServer])if(server)await terminate(server.child)
  if(freshCreated)sql('postgres',`DROP DATABASE ${fresh} WITH (FORCE)`)
  if(rollbackCreated)sql('postgres',`DROP DATABASE ${rollback} WITH (FORCE)`)
  if(created)sql('postgres',`DROP DATABASE ${db} WITH (FORCE)`)
 }
}
