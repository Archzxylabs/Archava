/** Native financial review helpers; no direct business writes or grant changes. */
import assert from 'node:assert/strict'
export function financialUi({page,origin,until,company}){
 const t=id=>`[data-testid="${id}"]`
 const select=async(id,value)=>{
  await until(()=>page.evaluate(`!![...document.querySelector(${JSON.stringify(t(id))})?.options||[]].find(o=>o.value===${JSON.stringify(String(value))})`),id+' current option')
  await page.evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(t(id))});n.value=${JSON.stringify(String(value))};n.dispatchEvent(new Event('change',{bubbles:true}));})()`)
 }
 const open=async()=>{await page.navigate(`${origin}/app/erp`);await until(()=>page.evaluate(`!!document.querySelector('${t('ops-tab-accounting')}')`),'visible Accounting navigation');await page.click(t('ops-tab-accounting'));await until(()=>page.evaluate(`new URL(location.href).searchParams.get('module')==='accounting'`),'Accounting tab route');await select('trade-company-select',company);await until(()=>page.evaluate(`!!document.querySelector('${t('operator-action')}')`),'current financial workspace')}
 const review=async(type,fields)=>{
  await select('operator-action',type);await until(()=>page.evaluate("document.querySelector('.operator-actions form fieldset')?.disabled===false"),'current financial grant')
  for(const[k,value]of Object.entries(fields)){
   const id=`operator-accounting-${k}`;await until(()=>page.evaluate(`!!document.querySelector(${JSON.stringify(t(id))})`),id+' rendered')
   const kind=await page.evaluate(`document.querySelector(${JSON.stringify(t(id))}).tagName`)
   if(kind==='SELECT')await select(id,value);else await page.fill(t(id),String(value))
  }
  await page.click(t('module-accounting-create'));await until(()=>page.evaluate(`!!document.querySelector('${t('operator-review')}')`),'explicit financial review')
  assert.equal(await page.evaluate(`document.querySelector('${t('operator-confirm')}').disabled`),false,'Current source, accounts and exact amounts must permit confirmation')
 }
 const confirm=async()=>{await page.click(t('operator-confirm'));await until(async()=>{if(await page.evaluate(`!!document.querySelector('${t('ops-refused')}')||!!document.querySelector('${t('ops-uncertain')}')`))throw Error(await page.text(t('ops-feedback')));return(await page.text(t('ops-feedback'))).includes('Saved.')},'matching financial receipt')}
 const action=async(type,fields)=>{await review(type,fields);await confirm()}
 return{t,select,open,review,confirm,action}
}
