/** App-owned pending references must survive separate tabs, retries and sign-out. */
import assert from 'node:assert/strict'

export async function verifyPendingRecovery({page,openTab,origin,until,pass,suffix,login}) {
 const t=id=>`[data-testid="${id}"]`
 const markers=()=>page.evaluate(`Object.keys(localStorage).filter(k=>k.startsWith('archava.pending.')).map(k=>({key:k,...JSON.parse(localStorage.getItem(k))}))`)
 const fill=async(tab,name,email)=>{
  await tab.evaluate("document.querySelector('.contact-create').open=true")
  await tab.fill(t('contact-new-name'),name);await tab.fill(t('contact-new-email'),email)
 }
 const fault=async(tab,afterCommit)=>tab.evaluate(`(()=>{const original=window.fetch.bind(window);window.fetch=async function(input,init){const request=input instanceof Request?input:new Request(input,init);if(request.method==='POST'&&new URL(request.url).pathname==='/crm/commands'){window.__recoveryBody=await request.clone().json();window.fetch=original;${afterCommit?'await original(input,init);':''}throw new TypeError('Synthetic recovery transport loss');}return original(input,init);};})()`)
 const unknown=tab=>until(()=>tab.evaluate("!!document.querySelector('[data-testid=contact-create-recover]')"),'contact retains original reference')
 const saved=tab=>until(()=>tab.text(t('contact-create-status')).then(s=>s.includes('Recorded')),'contact matching receipt')
 await page.navigate(`${origin}/app/contacts`)
 const other=await openTab()
 try {
  await other.page.navigate(`${origin}/app/contacts`)
  for(const tab of [page,other.page]) await until(()=>tab.evaluate("!!document.querySelector('[data-testid=contacts-count]') && !document.querySelector('[data-testid=contact-create]').disabled"),'both tabs restored before either request starts')
  const first={name:`Recovery A ${suffix}`,email:`recovery-a-${suffix}@example.test`}
  const second={name:`Recovery B ${suffix}`,email:`recovery-b-${suffix}@example.test`}
  await fill(page,first.name,first.email);await fill(other.page,second.name,second.email)
  await fault(page,true);await fault(other.page,false)
  await page.click(t('contact-create'));await unknown(page)
  await other.page.click(t('contact-create'));await unknown(other.page)
  const a=await page.evaluate('window.__recoveryBody.requestId')
  const b=await other.page.evaluate('window.__recoveryBody.requestId')
  assert.notEqual(a,b)
  assert.deepEqual((await markers()).map(m=>m.requestId).sort(),[a,b].sort())
  await page.click(t('contact-create-review-retry'));await page.fill(t('contact-new-name'),'Conflicting retry payload');await page.click(t('contact-create'));await unknown(page)
  assert.deepEqual((await markers()).map(m=>m.requestId).sort(),[a,b].sort(),'409 on a resumed request must preserve both pending references')
  await page.click(t('contact-create-review-retry'));await page.fill(t('contact-new-name'),first.name);await page.click(t('contact-create'));await saved(page)
  assert.deepEqual((await markers()).map(m=>m.requestId),[b],'acknowledging one tab must leave the other request intact')
  pass('CRM-two-tabs-and-conflicting-retry','Two independently opened contact forms retain separate keys; a conflicting retry is refused without losing either key; exact replay clears only its own marker')
  assert.deepEqual(other.page.errors,[])
  await page.click(t('session-sign-out'));await until(()=>page.evaluate("!!document.querySelector('[data-testid=login-submit]')"),'real logout completes')
  assert.deepEqual((await markers()).map(m=>m.requestId),[b],'logout must retain unresolved references')
  await login('alice')
  await unknown(page);assert.ok((await page.text(t('contact-create-status'))).includes(b))
  await page.click(t('contact-create-review-retry'));await fill(page,second.name,second.email);await page.click(t('contact-create'));await saved(page)
  assert.deepEqual(await markers(),[])
  for(const [key,entry] of [[a,first],[b,second]]) {
   const receipt=await page.evaluate(`fetch('/crm/commands/status?tenant=acme-retail&requestId=${key}').then(r=>r.json())`)
   assert.equal(receipt.requestId,key);assert.equal(receipt.status,'committed')
   const rows=await page.evaluate(`fetch('/contacts?tenant=acme-retail&search='+encodeURIComponent(${JSON.stringify(entry.name)})).then(r=>r.json())`)
   assert.equal(rows.items.length,1,'one real contact effect per original request');assert.equal(rows.items[0].id,receipt.receipt.id);assert.equal(rows.items[0].email,entry.email)
  }
  pass('CRM-before-commit-and-logout-recovery','A request dropped before reaching the server survives sign-out/login and review; the original key commits one contact, while the already committed request replays exactly once')
  // Legacy metadata from another account must neither block nor be removed by this account.
  await page.evaluate(`localStorage.setItem('archava.pending.acme-retail',JSON.stringify({requestId:'legacy-bob',tenant:'acme-retail',principal:'bob',createdAt:'2026-10-29T00:00:00Z'}))`)
  await page.navigate(`${origin}/app/contacts`)
  await until(()=>page.evaluate("!!document.querySelector('[data-testid=contact-create]') && !document.querySelector('[data-testid=contact-create]').disabled"),'foreign legacy marker does not block current account')
  assert.deepEqual((await markers()).map(m=>m.requestId),['legacy-bob'])
  await page.evaluate("localStorage.removeItem('archava.pending.acme-retail')")
  pass('CRM-pending-account-isolation','A legacy marker remains readable by its original account without blocking or being cleared by another account')
 } finally { await other.close() }
}
