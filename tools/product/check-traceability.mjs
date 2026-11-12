/** Validate inherited PRD inventory against its actual sources. No acceptance is inferred. */
import assert from 'node:assert/strict'
import {createHash} from 'node:crypto'
import {existsSync,readFileSync} from 'node:fs'
import {resolve,relative} from 'node:path'
import {fileURLToPath} from 'node:url'

export function checkTraceability(root){
 const read=path=>readFileSync(resolve(root,path),'utf8')
 const data=JSON.parse(read('docs/product/PRD_INHERITED_TRACEABILITY.v1.json'))
 const active=JSON.parse(read('docs/product/PRD_DELIVERY_REGISTER.v1.json'))
 const activeIds=new Set(active.requirements.map(r=>r.id)),ids=new Set(),sources=new Map()
 assert.equal(data.schemaVersion,1)
 const safe=path=>{assert.equal(typeof path,'string');assert.ok(!path.startsWith('/')&&!relative(root,resolve(root,path)).startsWith('..'));assert.ok(existsSync(resolve(root,path)),`Missing evidence ${path}`)}
 for(const source of data.sourceSnapshots){safe(source.path);const content=read(source.path);assert.equal(createHash('sha256').update(content).digest('hex'),source.sha256,`Source changed; review traceability: ${source.path}`);sources.set(source.path,content.split('\n'))}
 for(const r of data.requirements){
  assert.ok(!ids.has(r.id),`Duplicate inherited obligation ${r.id}`);ids.add(r.id)
  assert.ok(sources.has(r.source)&&sources.get(r.source)[r.line-1],`Missing original criterion ${r.id}`)
  assert.ok(['open','partial','accepted','superseded','not_applicable'].includes(r.status))
  assert.ok(Array.isArray(r.remaining)&&Array.isArray(r.evidence)&&Array.isArray(r.mappedRequirements))
  for(const id of r.mappedRequirements)assert.ok(activeIds.has(id),`Unknown active mapping ${id}`)
  for(const e of r.evidence)safe(e)
  if(r.status==='accepted')assert.ok(r.acceptance?.verified===true&&r.acceptance?.profileId&&r.remaining.length===0&&r.evidence.length>0,`Missing full acceptance ${r.id}`)
  if(r.status==='superseded'){assert.ok(r.supersession?.governingSource&&r.supersession?.criterion&&r.supersession?.reviewed===true);safe(r.supersession.governingSource)}
  if(r.status==='not_applicable'){assert.equal(r.scopeDecision?.ownerConfirmed,true);safe(r.scopeDecision.evidence)}
 }
 for(let i=0;i<=11;i++)assert.ok(ids.has(`RUST-R${String(i).padStart(2,'0')}`),'Migration milestone omitted')
 for(let i=1;i<=10;i++){assert.ok(ids.has(`RUST-G${String(i).padStart(2,'0')}`),'Migration gate omitted');assert.ok(ids.has(`OWN-T${String(i).padStart(2,'0')}`),'Owner acceptance omitted')}
 for(let i=1;i<=6;i++)assert.ok(ids.has(`EXEC-${String(i).padStart(2,'0')}`),'Inherited execution contract omitted')
 for(let i=1;i<=7;i++)assert.ok(ids.has(`B0${i}`),'Benchmark protocol omitted')
 const master='docs/archive/history/PRD.md'
 for(const line of sources.get(master)??[]){
  const named=/^\*\*([A-Z]+-\d\d):\*\*/.exec(line)
  if(named)assert.ok(ids.has(`MASTER-${named[1]}`),`Retained master contract omitted: ${named[1]}`)
 }
 const chapters=new Set()
 for(const c of data.chapterReview){assert.ok(sources.has(c.source));assert.equal(sources.get(c.source)[c.line-1],`## ${c.heading}`);const key=`${c.source}:${c.line}`;assert.ok(!chapters.has(key));chapters.add(key);for(const e of c.evidence)safe(e)
  if(c.classification!=='requires_scope_review'){assert.ok(['requirements_mapped','source_context','explicitly_superseded'].includes(c.classification));assert.equal(c.reviewed,true);assert.ok(c.evidence.length>0);if(c.classification==='requirements_mapped')assert.ok(c.mappedRequirements.length>0)}
 }
 for(const [path,lines] of sources)for(let i=0;i<lines.length;i++)if(lines[i].startsWith('## '))assert.ok(chapters.has(`${path}:${i+1}`),`Inherited chapter omitted: ${path}:${i+1}`)
 const remaining=data.requirements.filter(r=>!['accepted','superseded','not_applicable'].includes(r.status)).map(r=>r.id)
 const scopeReviews=data.chapterReview.filter(c=>c.classification==='requires_scope_review').length
 const complete=remaining.length===0&&scopeReviews===0
 if(data.status==='accepted')assert.ok(complete,'Inherited completion hides required work')
 return {schemaValid:true,inheritedComplete:complete,sources:sources.size,namedCriteria:ids.size,openCriteria:remaining,chapters:chapters.size,chapterReviewsRemaining:scopeReviews,notAPercentage:true}
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 try{process.stdout.write(JSON.stringify(checkTraceability(process.cwd()))+'\n')}catch(e){process.stderr.write(JSON.stringify({error:e.message})+'\n');process.exitCode=1}
}
