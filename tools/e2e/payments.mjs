/** Synthetic owned gateway. The production config cannot select this endpoint. */
import assert from 'node:assert/strict'
import { createServer } from 'node:http'
import { createHash } from 'node:crypto'
const key='synthetic-browser-payment-key'
const signature=(order,gross)=>createHash('sha512').update(`${order}200${gross}${key}`).digest('hex')
export async function startPaymentFixture(){
 const state={order:'',amount:0,status:'pending',refund:0,creates:0,checks:0}
 let origin
 const server=createServer(async(req,res)=>{
  try{
   assert.equal(req.headers.authorization,`Basic ${Buffer.from(`${key}:`).toString('base64')}`)
   let body='';for await(const chunk of req){body+=chunk;assert.ok(body.length<16384)}
   let answer
   if(req.method==='POST'&&req.url==='/snap/v1/transactions'){
    const payload=JSON.parse(body);state.order=payload.transaction_details.order_id;state.amount=payload.transaction_details.gross_amount;state.creates++
    answer={token:'owned-browser-token',redirect_url:`${origin}/snap/owned-browser-token`}
   }else if(req.method==='GET'&&/^\/v2\/[^/]+\/status$/.test(req.url)){
    state.checks++;const gross=`${state.amount}.00`;answer={order_id:state.order,transaction_id:'browser-transaction',status_code:'200',gross_amount:gross,currency:'IDR',fraud_status:'accept',transaction_status:state.status,signature_key:signature(state.order,gross)}
    if(state.refund>0)answer.refund_amount=`${state.refund}.00`
   }else{res.writeHead(404);res.end('{}');return}
   res.writeHead(200,{'content-type':'application/json'});res.end(JSON.stringify(answer))
  }catch{res.writeHead(500);res.end('{}')}
 })
 await new Promise((r)=>server.listen(0,'127.0.0.1',r));origin=`http://127.0.0.1:${server.address().port}`
 return {origin,state,close:async()=>{server.closeAllConnections();await new Promise(r=>server.close(r))}}
}
export async function verifyPayments({page,origin,until,pass,suffix,provider}){
 const t=id=>`[data-testid="${id}"]`
 await page.navigate(`${origin}/app/erp?module=invoices`)
 await until(()=>page.evaluate(`!!document.querySelector('${t('invoice-save')}')`),'invoice workspace for payment')
 await page.fill(t('invoice-code'),`PAY-${suffix}`);await page.fill(t('invoice-customer'),'Synthetic payment customer');await page.fill(t('invoice-reference'),'provider-proof');await page.fill(t('invoice-description'),'Service');await page.fill(t('invoice-quantity'),'1');await page.fill(t('invoice-price'),'100')
 await page.click(t('invoice-add-line'));await page.click(t('invoice-save'))
 const invoice=await until(async()=>{const v=await page.evaluate("fetch('/erp/billing/workspace?tenant=acme-retail').then(r=>r.json())");return v.invoices?.find(i=>i.code===`PAY-${suffix}`)},'payment invoice committed')
 await until(()=>page.evaluate(`!!document.querySelector('${t(`ops-open-${invoice.id}`)}')`),'payment invoice row');await page.click(t(`ops-open-${invoice.id}`));await page.fill(t('invoice-reason'),'Issued for payment proof');await page.click(t('invoice-issue'))
 await until(()=>page.evaluate(`document.querySelector('${t('payment-create')}')?.disabled===false`),'configured payment link action')
 // Real creation commits; only the browser acknowledgement is discarded.
 await page.evaluate(`(()=>{const original=window.fetch.bind(window);window.fetch=async function(input,init){const request=input instanceof Request?input:new Request(input,init);const response=await original(input,init);if(request.method==='POST'&&new URL(request.url).pathname==='/erp/billing/payments'&&response.ok){window.fetch=original;throw new TypeError('Owned test: creation response lost after commit');}return response;};})()`)
 await page.click(t('payment-create'));await until(()=>page.text(t('invoice-payment')).then(s=>s.includes('outcome is unconfirmed')),'payment response uncertainty')
 assert.equal(provider.state.creates,1)
 await page.navigate(`${origin}/app/erp?module=invoices`);await until(()=>page.evaluate(`!!document.querySelector('${t(`ops-open-${invoice.id}`)}')`),'invoice after reload');await page.click(t(`ops-open-${invoice.id}`));await until(()=>page.evaluate(`!!document.querySelector('${t('payment-recover')}')`),'durable payment pending marker')
 await page.click(t('payment-recover'));await until(()=>page.evaluate(`!!document.querySelector('${t('payment-checkout')}')&&!document.querySelector('${t('payment-recover')}')`),'original checkout recovered without resend');assert.equal(provider.state.creates,1)
 const link=await page.evaluate(`(()=>{const a=document.querySelector('${t('payment-checkout')}');return {href:a.href,target:a.target,rel:a.rel}})()`);assert.equal(link.href,`${provider.origin}/snap/owned-browser-token`);assert.equal(link.target,'_blank');assert.match(link.rel,/noopener/)
 pass('PAY-UI-original-reference','Real HTTP creation + SQL commit; lost response survives reload and recovers the original link, one provider invocation')
 await page.click(t('payment-check'));await until(()=>page.text(t('invoice-payment')).then(s=>s.includes('Waiting for customer payment')),'pending gateway status')
 const read=()=>page.evaluate("fetch('/erp/billing/workspace?tenant=acme-retail').then(r=>r.json())")
 assert.equal((await read()).invoices.find(i=>i.id===invoice.id).settled,0)
 await until(()=>page.evaluate(`document.querySelector('${t('payment-check')}')?.disabled===false`),'pending status finished')
 await new Promise(r=>setTimeout(r,2100));provider.state.status='settlement'
 await page.click(t('payment-check'));await until(async()=>{const v=await read();return v.invoices.find(i=>i.id===invoice.id)?.settled===100},'verified gateway settlement')
 await until(()=>page.text(t('invoice-payment')).then(s=>s.includes('Gateway payment verified')),'verified payment shown')
 await page.screenshot('payment-verified-light.png');pass('PAY-UI-verified-settlement','Status queried from owned HTTP gateway; actual invoice settled 100 IDR without a client mark-paid action')
 await new Promise(r=>setTimeout(r,2100));provider.state.status='partial_refund';provider.state.refund=40
 await page.click(t('payment-check'));await until(async()=>{const v=await read();const i=v.invoices.find(i=>i.id===invoice.id);return i?.settled===60&&i?.outstanding===40},'verified cumulative refund')
 await until(()=>page.text(t('invoice-payment')).then(s=>s.includes('A verified refund')),'refund shown');await page.screenshot('payment-refund-light.png')
 pass('PAY-UI-refund','Authenticated cumulative refund 40 IDR reopens the invoice with settlement 60 and outstanding 40')
 return {invoice:invoice.id,order:provider.state.order,providerCreates:provider.state.creates,providerChecks:provider.state.checks}
}
