import test from 'node:test'
import assert from 'node:assert/strict'
import {mkdtempSync,mkdirSync,writeFileSync,readFileSync,rmSync} from 'node:fs'
import {join,dirname} from 'node:path'
import {tmpdir} from 'node:os'
import {checkDelivery} from '../product/check-delivery.mjs'
import {checkTraceability} from '../product/check-traceability.mjs'
function fixture(run){const root=mkdtempSync(join(tmpdir(),'archava-delivery-test-'));try{const data=JSON.parse(readFileSync('docs/product/PRD_DELIVERY_REGISTER.v1.json','utf8'));data.overallStatus='in_progress';data.releaseProfile.status='needs_business_input';data.inheritedRequirements.status='requires_traceability';delete data.inheritedRequirements.inventory;for(const r of data.requirements){r.status='open';r.evidence=[];r.remaining=['Independent acceptance is still required'];delete r.acceptance;delete r.scopeDecision}const save=()=>writeFileSync(join(root,'docs/product/PRD_DELIVERY_REGISTER.v1.json'),JSON.stringify(data));for(const p of new Set(['docs/product/PRD.md',...data.requirements.map(r=>r.source)])){mkdirSync(dirname(join(root,p)),{recursive:true});writeFileSync(join(root,p),readFileSync(p))}save();return run({root,data,save})}finally{rmSync(root,{recursive:true,force:true})}}
test('a passing local milestone cannot complete open whole-product requirements',()=>fixture(({root,data,save})=>{assert.equal(checkDelivery(root).wholeProductComplete,false);assert.throws(()=>checkDelivery(root,true),/acceptance remains open/);data.overallStatus='complete';save();assert.throws(()=>checkDelivery(root),/cannot hide open/)}))
test('omitted requirements and missing evidence are refused',()=>fixture(({root,data,save})=>{data.requirements=data.requirements.filter(r=>r.id!=='FIN-01');save();assert.throws(()=>checkDelivery(root),/omitted: FIN-01/)}))
test('accepted and excluded requirements need actual acceptance or an explicit owner decision',()=>fixture(({root,data,save})=>{data.requirements[0].status='accepted';save();assert.throws(()=>checkDelivery(root),/Acceptance lacks proof/);data.requirements[0].status='not_applicable';save();assert.throws(()=>checkDelivery(root),/No explicit owner scope/)}))

test('an inherited acceptance flag cannot replace its required source-backed proofs',()=>fixture(({root,data,save})=>{data.inheritedRequirements.status='accepted';save();assert.throws(()=>checkDelivery(root),/Inherited acceptance requires complete source-backed traceability/)}))

test('a retained named master contract cannot disappear behind reviewed chapter metadata',()=>{
 const root=mkdtempSync(join(tmpdir(),'archava-master-contract-test-'))
 try{
  const path='docs/product/PRD_INHERITED_TRACEABILITY.v1.json',data=JSON.parse(readFileSync(path,'utf8'))
  const files=new Set(['docs/product/PRD_DELIVERY_REGISTER.v1.json',...data.sourceSnapshots.map(s=>s.path),...data.requirements.flatMap(r=>r.evidence),...data.chapterReview.flatMap(c=>c.evidence)])
  for(const file of files){mkdirSync(dirname(join(root,file)),{recursive:true});writeFileSync(join(root,file),readFileSync(file))}
  const save=()=>writeFileSync(join(root,path),JSON.stringify(data));save()
  assert.equal(checkTraceability(root).inheritedComplete,false)
  assert.ok(data.requirements.some(r=>r.id==='MASTER-AUTH-04'))
  data.requirements=data.requirements.filter(r=>r.id!=='MASTER-AUTH-04');save()
  assert.throws(()=>checkTraceability(root),/Retained master contract omitted: AUTH-04/)
 }finally{rmSync(root,{recursive:true,force:true})}
})
