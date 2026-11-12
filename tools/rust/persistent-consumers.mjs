/** Cold public clients plus disposable, independently configured native databases. */
import assert from 'node:assert/strict'
import {spawnSync} from 'node:child_process'
import {existsSync,mkdirSync,mkdtempSync,readFileSync,writeFileSync,copyFileSync,chmodSync,rmSync} from 'node:fs'
import {tmpdir} from 'node:os'
import {join,resolve,relative} from 'node:path'
import {fileURLToPath} from 'node:url'
import {sourceManifest,sha256} from './provenance.mjs'
import {verifyPersistentConsumers} from '../consumers/persistent/verify.mjs'
const root=fileURLToPath(new URL('../../',import.meta.url)),args=process.argv.slice(2)
if(args.length===1&&args[0]==='--help'){
 process.stdout.write('node tools/rust/persistent-consumers.mjs --native VERIFIED_NATIVE_PACKAGE --output NEW_DIRECTORY\nBuilds two independent public core/domain clients offline, then verifies scoped persistent CRM, reviewed Owner labels, project/order extensions, current authority, restart and installation isolation on disposable PostgreSQL/native hosts. Requires cached Cargo dependencies, tar and PostgreSQL binaries. No shared database or public service is used. Not full K04/K05 customer acceptance.\n');process.exit(0)
}
assert.equal(args.length,4,'Use --native PACKAGE --output NEW_DIRECTORY');assert.equal(args[0],'--native');assert.equal(args[2],'--output')
const pkg=resolve(args[1]),out=resolve(args[3]);assert.ok(!existsSync(out),'Preserve earlier evidence: output already exists');mkdirSync(out,{recursive:true})
const env=Object.fromEntries(['PATH','HOME','CARGO_HOME','RUSTUP_HOME'].filter(k=>process.env[k]).map(k=>[k,process.env[k]]));Object.assign(env,{RUSTC_WRAPPER:'',RUSTUP_TOOLCHAIN:'1.96.0',CARGO_INCREMENTAL:'0'})
const source=sourceManifest(root),result={schemaVersion:1,scope:'Two independently built public core/domain clients followed by configured disposable native database tests. Not full industry-kit or customer acceptance.',sourceSha256:source.sha256,toolSha256:sha256(fileURLToPath(import.meta.url)),verified:false,archives:[],consumers:[]}
let scratch
function run(bin,arguments_,cwd,log,extra={}){
 const r=spawnSync(bin,arguments_,{cwd,env:{...env,...extra},encoding:'utf8',timeout:600000,maxBuffer:16*1024*1024});writeFileSync(join(out,log),(r.stdout??'')+(r.stderr??''));assert.equal(r.error,undefined,`${log}: ${r.error?.message}`);assert.equal(r.signal,null,`${log}: ${r.signal}`);assert.equal(r.status,0,`${log}: exit ${r.status}`);return r.stdout
}
try{
 const profile=JSON.parse(readFileSync(join(pkg,'release-profile.json'),'utf8'));assert.equal(profile.distribution,'native')
 for(const a of profile.artifacts){const path=resolve(pkg,a.tarball);assert.ok(path.startsWith(pkg+'/'),'Native artifact escapes package');assert.equal(sha256(path),a.sha256,`Native artifact differs: ${a.name}`)}
 const built=JSON.parse(readFileSync(join(pkg,'build-provenance.json'),'utf8'));assert.equal(built.source.sha256,profile.sourceSha256)
 const portable=manifest=>manifest.inputs.filter(v=>v.path.startsWith('crates/core/')||v.path.startsWith('crates/domain/')||['Cargo.toml','Cargo.lock','rust-toolchain.toml','.cargo/config.toml'].includes(v.path))
 assert.deepEqual(portable(source),portable(built.source),'Current public-contract sources must equal the verified native package inputs')
 result.nativeProfileSha256=sha256(join(pkg,'release-profile.json'));result.nativeSourceSha256=profile.sourceSha256;result.nativePortableInputsMatch=true
 run(join(pkg,'bin/archava-ops'),['verify-release'],pkg,'native-verification.json',{ARCHAVA_OPS_ROOT:pkg})
 run('cargo',['package','-p','archava-core','-p','archava-domain','--locked','--offline','--allow-dirty','--no-verify','--exclude-lockfile','--target-dir',join(out,'packages')],root,'package.log')
 for(const name of ['archava-core','archava-domain']){const path=join(out,'packages/package',`${name}-0.1.0.crate`);result.archives.push({name,path:relative(root,path),sha256:sha256(path)})}
 scratch=mkdtempSync(join(tmpdir(),'archava-independent-clients-'))
 for(const model of ['agency','wholesale']){
  const project=join(scratch,model),vendor=join(project,'vendor'),evidence=join(out,model);mkdirSync(vendor,{recursive:true});mkdirSync(join(project,'src'));mkdirSync(evidence)
  for(const archive of result.archives){const path=resolve(root,archive.path);assert.equal(sha256(path),archive.sha256);run('tar',['-xzf',path,'-C',vendor,'--no-same-owner','--no-same-permissions'],project,`${model}-extract-${archive.name}.log`)}
  const manifest=`[package]\nname="archava-independent-${model}-client"\nversion="0.0.0"\nedition="2021"\nrust-version="1.96"\n[workspace]\n[dependencies]\narchava-core={version="=0.1.0",path="vendor/archava-core-0.1.0"}\narchava-domain={version="=0.1.0",path="vendor/archava-domain-0.1.0"}\nserde={version="=1.0.229",features=["derive"]}\nserde_json="=1.0.154"\n[patch.crates-io]\narchava-core={path="vendor/archava-core-0.1.0"}\n`
  writeFileSync(join(project,'Cargo.toml'),manifest);writeFileSync(join(evidence,'Cargo.toml'),manifest)
  for(const [from,to]of [[`${model}.rs`,'main.rs'],['shared.rs','shared.rs']]){const input=join(root,'tools/consumers/persistent',from);copyFileSync(input,join(project,'src',to));copyFileSync(input,join(evidence,to))}
  const extra={CARGO_TARGET_DIR:join(project,'build')}
  run('cargo',['generate-lockfile','--offline'],project,`${model}/resolve.log`,extra);copyFileSync(join(project,'Cargo.lock'),join(evidence,'Cargo.lock'))
  const tree=run('cargo',['tree','--locked','--offline','--edges','normal','--prefix','none'],project,`${model}/tree.log`,extra)
  for(const name of ['archava-host','archava-adapters','archava-slice','sqlx','axum','leptos','reqwest','tokio'])assert.ok(!new RegExp('^'+name+'(?: |-)','m').test(tree),`Forbidden client dependency: ${name}`)
  assert.match(run('cargo',['test','--release','--locked','--offline','--','--test-threads=1'],project,`${model}/tests.log`,extra),/test result: ok\. 2 passed; 0 failed; 0 ignored;/)
  run('cargo',['build','--release','--locked','--offline'],project,`${model}/build.log`,extra);copyFileSync(join(project,'build/release',`archava-independent-${model}-client`),join(evidence,'client'));chmodSync(join(evidence,'client'),0o755)
  const labels={new:model==='agency'?'Discovery':'Buyer enquiry',qualified:model==='agency'?'Scope confirmed':'Buyer qualified',proposal:model==='agency'?'Proposal review':'Quote sent',won:model==='agency'?'Project accepted':'Order accepted',lost:'Declined'}
  writeFileSync(join(evidence,'config-prototype.json'),JSON.stringify({tenant:`${model}-client`,model,configRevision:1,pipelineLabels:labels})+'\n')
  result.consumers.push({name:model,tests:2,binarySha256:sha256(join(evidence,'client')),mainSha256:sha256(join(evidence,'main.rs')),sharedSha256:sha256(join(evidence,'shared.rs')),manifestSha256:sha256(join(evidence,'Cargo.toml')),lockSha256:sha256(join(evidence,'Cargo.lock')),independentRoot:true,independentTarget:true,workspaceImports:false,hostLibraries:false})
 }
 assert.equal(sourceManifest(root).sha256,source.sha256,'Serving inputs changed while clients were built');result.verified=true
}catch(e){result.error=e.stack;process.exitCode=1}
finally{if(scratch)rmSync(scratch,{recursive:true,force:true});result.ownedScratchRemoved=true;writeFileSync(join(out,'build-result.json'),JSON.stringify(result,null,2)+'\n')}
let persistent
if(result.verified){persistent=await verifyPersistentConsumers({root,out,pkg});if(!persistent.verified||persistent.postgresCleanupExitCode!==0)process.exitCode=1}
const completed={schemaVersion:1,scope:result.scope,verified:result.verified&&persistent?.verified===true&&persistent?.postgresCleanupExitCode===0,sourceSha256:result.sourceSha256,nativeSourceSha256:result.nativeSourceSha256,buildResultSha256:sha256(join(out,'build-result.json')),...(persistent?{persistentResultSha256:sha256(join(out,'persistent-result.json')),checks:persistent.checks.length,postgresCleanupExitCode:persistent.postgresCleanupExitCode}:{}),ownedScratchRemoved:result.ownedScratchRemoved&&persistent?.ownedScratchRemoved===true,error:result.error??persistent?.error}
writeFileSync(join(out,'result.json'),JSON.stringify(completed,null,2)+'\n')
process.stdout.write(JSON.stringify(completed)+'\n')
