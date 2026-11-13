/** Cold public-crate consumers. No application DB, network, private env, or source links. */
import assert from 'node:assert/strict'
import {spawnSync} from 'node:child_process'
import {createHash} from 'node:crypto'
import {existsSync,mkdirSync,mkdtempSync,readFileSync,rmSync,writeFileSync} from 'node:fs'
import {tmpdir} from 'node:os'
import {join,relative,resolve} from 'node:path'
import {fileURLToPath} from 'node:url'
import {sourceManifest,sha256} from './provenance.mjs'
const root=fileURLToPath(new URL('../../',import.meta.url)),args=process.argv.slice(2)
if(args.length===1&&args[0]==='--help'){
 process.stdout.write('node tools/rust/cold-consumers.mjs --output NEW_DIRECTORY\nPackages current core/domain once, then builds and runs separate K05 agency and K04 wholesale public-contract consumers offline in independent temporary roots. Requires cached Cargo dependencies and tar. Does not certify persistence, configured kits, UI, or full customer workflows.\n')
 process.exit(0)
}
assert.equal(args.length,2,'Use --output NEW_DIRECTORY');assert.equal(args[0],'--output')
const out=resolve(args[1]);assert.ok(!existsSync(out),'Output already exists; preserve prior evidence');mkdirSync(out,{recursive:true})
const source=sourceManifest(root),result={schemaVersion:1,scope:'Two cold consumers of identical packaged core/domain contracts, with different labels and project/commerce extensions. This does not certify host persistence, configurable kits, user workflows, or every CFG-03/RUST-G05 obligation.',sourceSha256:source.sha256,toolSha256:sha256(fileURLToPath(import.meta.url)),consumers:[],archives:[],verified:false}
const hash=value=>createHash('sha256').update(value).digest('hex')
const environment={PATH:process.env.PATH,HOME:process.env.HOME,RUSTUP_TOOLCHAIN:'1.96.0',RUSTC_WRAPPER:'',CARGO_INCREMENTAL:'0',...(process.env.CARGO_HOME?{CARGO_HOME:process.env.CARGO_HOME}:{}),...(process.env.RUSTUP_HOME?{RUSTUP_HOME:process.env.RUSTUP_HOME}:{})}
const run=(command,args,cwd,log,extra={})=>{
 const r=spawnSync(command,args,{cwd,env:{...environment,...extra},encoding:'utf8',timeout:600000,maxBuffer:16*1024*1024})
 writeFileSync(join(out,log),(r.stdout||'')+(r.stderr||''))
 assert.equal(r.error,undefined,`${log}: ${r.error?.message}`);assert.equal(r.signal,null,`${log}: signal ${r.signal}`);assert.equal(r.status,0,`${log}: exit ${r.status}`)
 return r.stdout
}
let scratch
try{
 run('cargo',['package','-p','archava-core','-p','archava-domain','--locked','--offline','--allow-dirty','--no-verify','--exclude-lockfile','--target-dir',join(out,'packages')],root,'package.log')
 for(const name of ['archava-core','archava-domain']){const path=join(out,'packages/package',`${name}-0.1.0.crate`);assert.ok(existsSync(path));result.archives.push({name,path:relative(root,path),sha256:sha256(path)})}
 scratch=mkdtempSync(join(tmpdir(),'archava-cold-consumers-'))
 for(const name of ['agency','wholesale']){
  const dir=join(scratch,name),vendor=join(dir,'vendor');mkdirSync(vendor,{recursive:true});mkdirSync(join(dir,'src'))
  for(const archive of result.archives){
   const path=resolve(root,archive.path);assert.equal(sha256(path),archive.sha256)
   run('tar',['-xzf',path,'-C',vendor,'--no-same-owner','--no-same-permissions'],dir,`${name}-extract-${archive.name}.log`)
  }
  const manifest=`[package]\nname = "archava-cold-${name}"\nversion = "0.0.0"\nedition = "2021"\nrust-version = "1.96"\n[workspace]\n[dependencies]\narchava-core = { version = "=0.1.0", path = "vendor/archava-core-0.1.0" }\narchava-domain = { version = "=0.1.0", path = "vendor/archava-domain-0.1.0" }\nserde_json = "=1.0.154"\n[patch.crates-io]\narchava-core = { path = "vendor/archava-core-0.1.0" }\n`
  const fixture=readFileSync(join(root,'tools/consumers',name,'main.rs'))
  writeFileSync(join(dir,'Cargo.toml'),manifest);writeFileSync(join(dir,'src/main.rs'),fixture)
  writeFileSync(join(out,`${name}-Cargo.toml`),manifest);writeFileSync(join(out,`${name}-main.rs`),fixture)
  const env={CARGO_TARGET_DIR:join(dir,'build')}
  run('cargo',['generate-lockfile','--offline'],dir,`${name}-resolve.log`,env)
  writeFileSync(join(out,`${name}-Cargo.lock`),readFileSync(join(dir,'Cargo.lock')))
  const tree=run('cargo',['tree','--locked','--offline','--edges','normal','--prefix','none'],dir,`${name}-tree.log`,env)
  for(const forbidden of ['archava-host','archava-adapters','archava-slice','axum','sqlx','leptos','reqwest','tokio'])assert.ok(!new RegExp(`^${forbidden}(?: |-)`,'m').test(tree),`Unexpected consumer dependency: ${forbidden}`)
  assert.deepEqual([...new Set([...tree.matchAll(/^(archava-[a-z-]+) v/gm)].map(m=>m[1]))].sort(),['archava-cold-'+name,'archava-core','archava-domain'].sort())
  const tests=run('cargo',['test','--release','--locked','--offline','--','--test-threads=1'],dir,`${name}-tests.log`,env)
  assert.match(tests,/test result: ok\. 6 passed; 0 failed; 0 ignored;/)
  const output=run('cargo',['run','--release','--locked','--offline'],dir,`${name}-run.log`,env)
  result.consumers.push({name,kit:name==='agency'?'K05':'K04',fixtureSha256:hash(fixture),manifestSha256:hash(manifest),lockSha256:sha256(join(dir,'Cargo.lock')),treeSha256:hash(tree),tests:6,ignored:0,exitCode:0,output:output.trim(),shippedArchives:result.archives.map(a=>({name:a.name,sha256:a.sha256})),independentRoot:true,independentTarget:true,workspaceSourceImports:false,privateEnvCopied:false,network:false,applicationDatabase:false})
 }
 assert.equal(sourceManifest(root).sha256,source.sha256,'Serving inputs changed during proof')
 result.verified=true
}catch(e){result.error=e.stack;process.exitCode=1}
finally{
 if(scratch)rmSync(scratch,{recursive:true,force:true})
 result.ownedScratchRemoved=true;writeFileSync(join(out,'result.json'),JSON.stringify(result,null,2)+'\n')
 process.stdout.write(JSON.stringify({verified:result.verified,consumers:result.consumers.length,sourceSha256:result.sourceSha256,error:result.error})+'\n')
}
