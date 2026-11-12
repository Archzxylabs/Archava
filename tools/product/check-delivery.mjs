/** Whole-PRD status is distinct from a passing bounded feature or build. No services. */
import assert from 'node:assert/strict'
import {readFileSync,existsSync} from 'node:fs'
import {resolve,relative} from 'node:path'
import {fileURLToPath} from 'node:url'
import {checkTraceability} from './check-traceability.mjs'
export function checkDelivery(root,requireComplete=false){
 const path=resolve(root,'docs/product/PRD_DELIVERY_REGISTER.v1.json'),data=JSON.parse(readFileSync(path,'utf8')),source=readFileSync(resolve(root,'docs/product/PRD.md'),'utf8')
 assert.equal(data.schemaVersion,1);const ids=new Set(),statuses=new Set(['open','partial','accepted','not_applicable'])
 const safe=path=>{assert.equal(typeof path,'string');const file=resolve(root,path);assert.ok(!relative(root,file).startsWith('..')&&!path.startsWith('/'),'Evidence must remain in the repository');assert.ok(existsSync(file),`Missing evidence ${path}`)}
 for(const r of data.requirements){assert.ok(!ids.has(r.id),`Duplicate requirement ${r.id}`);ids.add(r.id);assert.ok(statuses.has(r.status),`Invalid status ${r.id}`);safe(r.source);assert.ok(Array.isArray(r.evidence)&&Array.isArray(r.remaining));for(const e of r.evidence)safe(e)
  if(r.status==='accepted'){assert.ok(r.evidence.length>0&&r.remaining.length===0,`Acceptance lacks proof or retains work: ${r.id}`);assert.ok(r.acceptance?.profileId&&r.acceptance?.verified===true,`Missing selected-profile acceptance ${r.id}`)}
  if(r.status==='not_applicable')assert.ok(r.scopeDecision?.ownerConfirmed===true&&r.scopeDecision?.evidence,`No explicit owner scope decision ${r.id}`)
 }
 for(const m of source.matchAll(/^\| ((?:DAT|FIN|AGT|CFG|UX|INT|SEC|OPS)-\d\d) \|/gm))assert.ok(ids.has(m[1]),`PRD requirement omitted: ${m[1]}`)
 assert.ok(ids.has('EXT-01'));for(let i=1;i<=8;i++)assert.ok(ids.has(`OWN-${String(i).padStart(2,'0')}`))
 let inherited=null
 if(data.inheritedRequirements.inventory){assert.equal(data.inheritedRequirements.inventory,'docs/product/PRD_INHERITED_TRACEABILITY.v1.json');safe(data.inheritedRequirements.inventory);inherited=checkTraceability(root)}
 if(data.inheritedRequirements.status==='accepted')assert.ok(inherited?.inheritedComplete===true,'Inherited acceptance requires complete source-backed traceability')
 const open=data.requirements.filter(r=>!['accepted','not_applicable'].includes(r.status)).map(r=>r.id),profileReady=data.releaseProfile.status==='accepted',inheritedReady=data.inheritedRequirements.status==='accepted'&&inherited?.inheritedComplete===true,complete=open.length===0&&profileReady&&inheritedReady
 if(data.overallStatus==='complete')assert.ok(complete,'Whole-product completion cannot hide open requirements, profile or inherited work')
 const result={schemaValid:true,wholeProductComplete:complete,requirements:data.requirements.length,openRequirements:open,releaseProfile:data.releaseProfile.status,inheritedRequirements:data.inheritedRequirements.status,inherited,source:'docs/product/PRD_DELIVERY_REGISTER.v1.json',notAPercentage:true}
 if(requireComplete&&!complete){const e=new Error('Whole PRD acceptance remains open');e.result=result;throw e}return result
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 try{const result=checkDelivery(process.cwd(),process.argv.includes('--require-complete'));process.stdout.write(JSON.stringify(result)+'\n')}catch(e){process.stderr.write(JSON.stringify({error:e.message,...e.result})+'\n');process.exitCode=e.result?2:1}
}
