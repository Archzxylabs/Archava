/** Installed non-owner role and freshly bootstrapped owner; no SQL grant repair. */
import assert from 'node:assert/strict'
import {randomUUID} from 'node:crypto'
export async function verifyNativePayables({server,session,tenant,call,sql,terms=false,pass}) {
  const ok=async promise=>{const r=await promise;assert.equal(r.status,200,JSON.stringify(r));return r.body.receipt??r.body}
  const mutate=(module,command)=>ok(call(server.origin,session,`/erp/${module}/commands`,{tenant,requestId:randomUUID(),command}))
  const finance=command=>mutate('accounting',command),org=(type,data)=>ok(call(server.origin,session,'/erp/organizations/commands',{tenant,mutation:{requestId:randomUUID(),type,data}}))
  const organization=await org('create_organization',{name:'Fresh native supplier payment',legalName:'Fresh Native Supplier Payment Company'})
  const company=await org('company.create_from_organization',{organizationId:organization.organizationId,expectedSourceRevision:1,code:'FRESH-PAYABLE',reason:'Verify installed supplier payment authority'})
  const book=await finance({type:'book.create',companyId:company.companyId,expectedCompanyRevision:1,code:'FRESH-PAYABLE',name:'Fresh supplier payment book',currency:'IDR',sourceAuthority:'native',independentPosting:false,reason:'Explicit synthetic single-operator policy'})
  const accounts={};for(const[code,cls,role]of [['INV','asset','operating'],['GRNI','liability','operating'],['AP','liability','operating'],['FEE','liability','operating'],['CASH','asset','cash']])accounts[code]=(await finance({type:'account.create',bookId:book.bookId,code,name:code,class:cls,cashRole:role,reason:'Explicit fresh native source accounts'})).resourceId
  const ref=code=>({accountId:accounts[code],expectedAccountRevision:1});let date=sql("SELECT (clock_timestamp() AT TIME ZONE 'UTC')::date::text")
  await finance({type:'period.open',bookId:book.bookId,code:'SOURCE',startDate:'2000-01-01',endDate:'2100-12-31',reason:'Owned actual-source window'})
  await finance({type:'purchase_policy.set',bookId:book.bookId,expectedRevision:0,currencyCatalogue:'SIX-2026-09-17',sourceUnit:'whole_idr',mappings:[['inventory','INV'],['goods_received_not_invoiced','GRNI'],['payable','AP'],['landed_fee_accrual','FEE']].map(([role,code])=>({role,...ref(code)})),reason:'Explicit installed native purchase capture'})
  const supplier=await mutate('purchasing',{operation:'supplier.create',code:'FRESH-PAYABLE',name:'Owned fresh native supplier'})
  const product=await mutate('commerce',{operation:'product.create_for_company',companyId:company.companyId,expectedCompanyRevision:1,sku:'FRESH-PAYABLE',title:'Actual fresh native goods',unit:'unit',trackStock:true,price:15000})
  const offer=await mutate('purchasing',{operation:'offer.create',supplierId:supplier.id,productId:product.id,unitCost:10000})
  const po=await mutate('purchasing',{operation:'order.create-for-company',companyId:company.companyId,expectedCompanyRevision:1,supplierId:supplier.id,lines:[{offerId:offer.id,quantity:1}]})
  await mutate('purchasing',{operation:'order.confirm',id:po.id,expectedRevision:1,reason:'Confirm real installed native source'})
  const line=sql(`SELECT id FROM archava_purchasing.lines WHERE tenant_id='${tenant}' AND order_id='${po.id}'`)
  const receipt=await mutate('purchasing',{operation:'order.receive',id:po.id,expectedRevision:2,lines:[{lineId:line,accepted:1,rejected:0}],reason:'Actual fresh accepted goods'})
  const doc=await mutate('purchasing',{operation:'document.capture',orderId:po.id,expectedOrderRevision:3,supplierId:supplier.id,vendorReference:'FRESH-PAYABLE',documentDate:date,currency:'IDR',declaredTotal:10000,lines:[{lineId:line,quantity:1,unitCost:10000}]})
  const bill=await mutate('purchasing',{operation:'document.match',id:doc.id,expectedRevision:1,expectedOrderRevision:3,reason:'Match real accepted supplier goods'})
  date=sql(`SELECT source_date::text FROM archava_accounting.purchase_sources WHERE tenant_id='${tenant}' AND id='${bill.requestId}'`)
  for(const[source,code,roles]of [[receipt.requestId,'RECEIPT',['INV','GRNI']],[bill.requestId,'BILL',['GRNI','AP']]]){
    const j=await finance({type:'purchase_source.draft',sourceId:source,code,accounts:roles.map(ref),reason:'Review actual source accounts with installed runtime grants'})
    await finance({type:'journal.post',id:j.resourceId,expectedRevision:1,reason:'Separate installed native source posting'})
  }
  const bank=await finance({type:'bank_account.create',bookId:book.bookId,code:'BANK',name:'Fresh actual payment evidence bank',account:ref('CASH'),reason:'Exercise installed manual evidence grants'})
  await finance({type:'bank_statement.import',bankAccountId:bank.resourceId,account:ref('CASH'),statement:{reference:'FRESH-PAYABLE',fromDate:date,toDate:date,currency:'IDR',openingMinor:'1000000',closingMinor:'0',rows:[{externalRef:'ACTUAL-SUPPLIER-PAYMENT',date,amountMinor:'-1000000',description:'Actual synthetic supplier withdrawal'}]},reason:'Explicit evidence; no bank transfer or invented opening journal'})
  const workspace=await ok(call(server.origin,session,`/erp/accounting/workspace?tenant=${tenant}&companyId=${company.companyId}&collection=bank_rows`))
  const row=workspace.bank_rows[0];assert.ok(row)
  const body={tenant,requestId:randomUUID(),command:{type:'payable_payment.draft',sourceId:bill.requestId,expectedDocumentRevision:2,bankRowId:row.id,code:'PAYMENT',payableAccount:ref('AP'),cashAccount:ref('CASH'),amountMinor:'1000000',reason:'Actual installed role creates reviewed supplier settlement'}}
  const payment=await ok(call(server.origin,session,'/erp/accounting/commands',body));assert.equal(payment.bankRowId,row.id);assert.equal(payment.sourceId,bill.requestId)
  await finance({type:'journal.post',id:payment.resourceId,expectedRevision:1,reason:'Separate installed native supplier payment posting'})
  await finance({type:'bank_allocation.record',bankRowId:row.id,journalId:payment.resourceId,journalLineOrdinal:2,expectedJournalRevision:2,account:ref('CASH'),amountMinor:'1000000',reason:'Match actual installed native supplier evidence'})
  const replay=await ok(call(server.origin,session,'/erp/accounting/commands',body));assert.equal(replay.replayed,true);assert.equal(replay.resourceId,payment.resourceId)
  const report=await ok(call(server.origin,session,`/erp/accounting/workspace?tenant=${tenant}&companyId=${company.companyId}&collection=payables`));assert.equal(report.payablesSummary.outstandingMinor,'0')
  let dueTerms=null
  if(terms){
    const page=await ok(call(server.origin,session,`/erp/accounting/workspace?tenant=${tenant}&companyId=${company.companyId}&collection=supplier_term_sources`))
    const source=page.supplier_term_sources.find(s=>s.id===bill.requestId);assert.ok(source);assert.equal(source.termsRevision,0)
    const agreed=new Date(`${source.sourceDate}T00:00:00Z`);agreed.setUTCDate(agreed.getUTCDate()+15);const dueDate=agreed.toISOString().slice(0,10)
    const saved=await finance({type:'supplier_terms.set',sourceId:source.id,expectedDocumentRevision:source.documentRevision,expectedRevision:0,effectiveFrom:source.sourceDate,condition:{kind:'due_on',date:dueDate},reason:'Actual installed owner reviews the original supplier due agreement'})
    assert.equal(saved.resourceKind,'document_terms');assert.equal(saved.documentTerms.condition.date,dueDate)
    const asOf=await ok(call(server.origin,session,`/erp/accounting/subledger-report?tenant=${tenant}&bookId=${book.bookId}&side=payable&asOf=${source.sourceDate}`))
    assert.equal(asOf.totals.netOutstandingMinor,'0');assert.equal(asOf.positions[0].terms.condition.date,dueDate);assert.equal(asOf.postedSourceControlsAgree,true)
    dueTerms={actualOriginalSupplierSource:true,installedOwnerWrites:true,installedNonOwnerRuntime:true,termsRevision:1,asOfReportVerified:true,extraAdministratorGrants:false}
    pass('OPS-fresh-native-reviewed-terms',dueTerms)
  }
  const result={dueTerms,actualPostedSupplierBill:true,actualBankEvidence:true,paymentPosted:true,bankMatched:true,originalReceiptReplayed:true,extraAdministratorGrants:false,openingJournalInferred:false,fullFinancialClose:false}
  pass('OPS-fresh-native-payable-grants',result);return result
}
