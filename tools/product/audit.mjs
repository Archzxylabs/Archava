/** Join product requirements, serving inputs and executed evidence. No services. */
import assert from 'node:assert/strict'
import { readFileSync, readdirSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { modules } from './catalogue.mjs'
import { sourceManifest, sha256 } from '../rust/provenance.mjs'

function block(text,opening) {
  let depth=0,string=false,escape=false,lineComment=false
  for(let i=opening;i<text.length;i++) {
    const c=text[i]
    if(lineComment){if(c==='\n')lineComment=false;continue}
    if(string){if(escape)escape=false;else if(c==='\\')escape=true;else if(c==='"')string=false;continue}
    if(c==='/'&&text[i+1]==='/'){lineComment=true;i++;continue}
    if(c==='"'){string=true;continue}
    if(c==='{')depth++
    if(c==='}'&&--depth===0)return text.slice(opening+1,i)
  }
  throw Error('Unclosed Rust declaration in product inventory')
}
const classes={accounting:'Command',companies:'CompanyCommand',parties:'PartyCommand',crm:'CrmCommand',commerce:'CommerceCommand',purchasing:'PurchaseCommand',support:'SupportCommand',organizations:'OrganizationKind',proposals:'ProposalKind',projects:'ProjectKind',foodservice:'FoodCommand',scheduling:'SchedulingCommand',accommodation:'AccommodationCommand',assets:'AssetCommand',billing:'BillingCommand',subscriptions:'SubscriptionCommand',attendance:'AttendanceCommand',laundry:'LaundryMutation',field:'FieldCommand',fulfillment:'FulfillmentMutation',education:'EducationCommand'}
export function operationInventory(source,id) {
  const klass=classes[id];assert.ok(klass,`Unknown business module ${id}`)
  const impl=new RegExp(`impl ${klass}\\s*\\{`).exec(source)
  if(impl) {
    const body=block(source,source.indexOf('{',impl.index))
    const method=/pub fn (?:operation|mutation_type|action|as_str)\b[^\{]*\{/.exec(body)
    if(method){const values=[...block(body,body.indexOf('{',method.index)).matchAll(/=>\s*"([a-z0-9_.-]+)"/g)].map(m=>m[1]);if(values.length)return [...new Set(values)]}
  }
  if(id==='foodservice') {
    const start=source.indexOf('pub fn command_operation(')
    if(start>=0)return [...new Set([...block(source,source.indexOf('{',start)).matchAll(/=>\s*"([a-z0-9_.-]+)"/g)].map(m=>m[1]))]
  }
  const enumeration=new RegExp(`pub enum ${klass}\\s*\\{`).exec(source)
  assert.ok(enumeration,`No command declaration for ${id}`)
  const body=block(source,source.indexOf('{',enumeration.index)), operations=[]
  let rename
  for(const line of body.split('\n')) {
    const named=/^    #\[serde\(rename = "([^"]+)"\)\]/.exec(line)
    if(named)rename=named[1]
    const variant=/^    ([A-Z][A-Za-z0-9]*)\s*(?:\{|,|\()/.exec(line)
    if(variant){operations.push(rename||variant[1].replace(/([a-z0-9])([A-Z])/g,'$1_$2').toLowerCase());rename=undefined}
  }
  assert.ok(operations.length,`No operation inventory for ${id}`);return operations
}
export function rustSuites(text) {
  const suites={};let active
  for(const line of text.split('\n')) {
    const run=/Running tests\/([a-z0-9_]+)\.rs\s/.exec(line)
    if(run){active={tests:[]};suites[run[1]]=active;continue}
    if(/^\s*(?:Running unittests|Doc-tests)/.test(line)){active=undefined;continue}
    const test=/^test (\S+) \.\.\. (ok|FAILED|ignored)$/.exec(line)
    if(active&&test)active.tests.push({name:test[1],outcome:test[2]})
    const end=/^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored/.exec(line)
    if(active&&end){Object.assign(active,{status:end[1],passed:Number(end[2]),failed:Number(end[3]),ignored:Number(end[4])});active=undefined}
  }
  return suites
}
export function audit({root,browserPath,rustLogPath,baselinePath}) {
  const read=path=>readFileSync(resolve(root,path),'utf8')
  const browser=JSON.parse(read(browserPath)), baseline=JSON.parse(read(baselinePath)), built=JSON.parse(read('target/site/build-provenance.json'))
  assert.equal(browser.status,'passed','Browser execution did not complete')
  assert.equal(browser.product?.auditCompleted,true,'Requirement probes did not complete')
  assert.equal(browser.fixtureCleanup.exitCode,0);assert.deepEqual(browser.pageErrors,[])
  const source=sourceManifest(root)
  assert.equal(source.sha256,baseline.sourceSha256,'Business/build inputs changed since the reused Rust evidence')
  assert.equal(source.sha256,built.source.sha256,'Serving build is stale')
  for(const artifact of browser.artifacts)assert.equal(artifact.sha256,sha256(resolve(root,artifact.path)))
  const suites=rustSuites(read(rustLogPath))
  const routeFiles=['crates/host/src/routes/erp.rs','crates/host/src/routes/crm.rs']
  const routes=routeFiles.flatMap(path=>[...read(path).matchAll(/\.route\(\s*"([^"]+)"\s*,\s*(get|post)\(/g)].map(m=>({path:m[1],method:m[2].toUpperCase(),source:path})))
    // The command pair of every module is generated from the registry, so it has no route literal to scan.
    .concat(modules.flatMap(module=>[['/commands','POST'],['/commands/status','GET']].map(([suffix,method])=>({path:`/erp/${module.id}${suffix}`,method,source:'config/modules.v1.json'}))))
  const uiFiles=readdirSync(resolve(root,'crates/slice/src')).filter(f=>f.endsWith('.rs')).map(f=>`crates/slice/src/${f}`).concat(readdirSync(resolve(root,'crates/slice/src/operations')).filter(f=>f.endsWith('.rs')).map(f=>`crates/slice/src/operations/${f}`))
  const uiMap={crm:['contacts.rs','contact.rs','draft.rs','desk.rs'],commerce:['commerce.rs'],purchasing:['purchasing.rs'],billing:['billing.rs','payments.rs','mail.rs'],support:['support.rs']}
  const domains=modules.map(module=>{
    const contract=module.id==='crm'?'crates/adapters/src/erp/crm.rs':`crates/domain/src/erp/${module.domain}.rs`
    const declaredOperations=operationInventory(read(contract),module.id).concat(module.id==='organizations'?operationInventory(read('crates/domain/src/erp/parties.rs'),'parties').concat(operationInventory(read('crates/domain/src/erp/companies.rs'),'companies')):[])
    const uiSource=uiFiles.filter(p=>uiMap[module.id]?.includes(p.split('/').at(-1))).map(read).join('\n')
    const tests=read(`crates/host/tests/${module.suite}.rs`), runtime=browser.product.domains.find(d=>d.id===module.id), suite=suites[module.suite]
    assert.ok(suite?.status==='ok'&&suite.failed===0&&suite.ignored===0,`Missing passing executed API evidence: ${module.suite}`)
    const prefix=module.id==='crm'?'/erp/crm/':`/erp/${module.id}/`
    return {...module,contract,declaredOperations,operationEvidence:declaredOperations.map(operation=>({operation,uiSourceReference:uiSource.includes(`"${operation}"`),apiTestSourceReference:tests.includes(`"${operation}"`),note:'Source references are not measured execution/branch coverage'})),routes:routes.filter(r=>r.path.startsWith(prefix)),apiExecution:{log:rustLogPath,suite:module.suite,...suite},browser:runtime}
  })
  const unmet=browser.product.requirements.filter(r=>r.status==='gap')
  return {schemaVersion:1,recordedAt:new Date().toISOString(),verdict:unmet.length?'incomplete':'observed_requirements_met',productComplete:unmet.length?false:null,source:{sha256:source.sha256,inputs:source.inputs.length,servingArtifactsMatch:true},scope:{businessDomains:domains.length,declaredOperations:domains.reduce((n,d)=>n+d.declaredOperations.length,0),runtimeProbes:browser.product.requirements.length,met:browser.product.counts.met,gaps:unmet.length,nativeOperatorDomains:domains.filter(d=>d.browser.screen.control).length,mountedCollectionReadDomains:domains.filter(d=>d.browser.read?.status===200).length,notAPercentage:true},evidence:{browser:browserPath,browserSha256:sha256(resolve(root,browserPath)),rustLog:rustLogPath,rustLogSha256:sha256(resolve(root,rustLogPath)),baseline:baselinePath,reusedRustEvidence:true,coreChecks:browser.checks.length,pageErrors:0,fixtureCleanupExitCode:0},domains,requirements:browser.product.requirements,unmet,limits:['Inventory covers the current bounded business contracts and explicit product journeys; future roadmap is not a shipped feature.','Per-operation source references do not prove that every branch was executed.','A mounted command/status API is not a complete operator workflow.','The browser runner succeeding means the audit completed; requirement gaps remain failures of product completeness.','Live providers, public deployment, actual-user acceptance and external consumers remain unverified.']}
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)) {
  const args=process.argv.slice(2), options={}
  for(let i=0;i<args.length;i+=2){assert.ok(['--browser','--rust-log','--baseline','--output'].includes(args[i])&&args[i+1]);options[args[i]]=args[i+1]}
  for(const name of ['--browser','--rust-log','--baseline','--output'])assert.ok(options[name],`Missing ${name}`)
  const result=audit({root:process.cwd(),browserPath:options['--browser'],rustLogPath:options['--rust-log'],baselinePath:options['--baseline']})
  writeFileSync(resolve(options['--output']),JSON.stringify(result,null,2)+'\n')
  process.stdout.write(JSON.stringify({verdict:result.verdict,scope:result.scope})+'\n')
  process.exitCode=result.verdict==='incomplete'?2:0
}
