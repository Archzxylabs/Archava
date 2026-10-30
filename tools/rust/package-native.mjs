/** Assemble a native distribution accepted by archava-ops, with source/artifact binding. */
import { execFileSync } from 'node:child_process'
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, renameSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { sha256, sourceManifest } from './provenance.mjs'
const root=resolve(dirname(fileURLToPath(import.meta.url)),'../..')
const args=process.argv.slice(2)
if(args.includes('--help')) { process.stdout.write('node tools/rust/package-native.mjs --out DIRECTORY\nRequires a fresh tools/rust/build-slice.mjs result. Builds the ops CLI, copies the native distribution, writes exact hashes, and verifies it. Refuses an existing output.\n');process.exit(0) }
if(args.length!==2||args[0]!=='--out')throw Error('Expected --out DIRECTORY')
const out=resolve(root,args[1]);if(existsSync(out))throw Error('Distribution output already exists; use a new candidate directory')
const built=JSON.parse(readFileSync(join(root,'target/site/build-provenance.json'),'utf8'))
if(built.source.sha256!==sourceManifest(root).sha256)throw Error('Source changed since the serving artifacts were built')
for(const artifact of built.artifacts)if(sha256(join(root,artifact.path))!==artifact.sha256)throw Error(`Serving artifact changed: ${artifact.path}`)
execFileSync('cargo',['build','--release','--locked','-p','archava-ops','--bin','archava-ops'],{cwd:root,stdio:'inherit'})
if(built.source.sha256!==sourceManifest(root).sha256)throw Error('Source changed while building operations CLI')
const staging=`${out}.staging-${process.pid}`;mkdirSync(staging,{recursive:true})
try{
 const copy=(from,to)=>{mkdirSync(dirname(join(staging,to)),{recursive:true});cpSync(join(root,from),join(staging,to),{recursive:true,errorOnExist:true,force:false})}
 copy('target/release/archava-server','bin/archava-server');copy('target/release/archava-ops','bin/archava-ops');copy('target/site/pkg','site/pkg');copy('target/site/build-provenance.json','build-provenance.json')
 copy('config/library-release.v1.json','config/library-release.v1.json');copy('config/payment-providers.example.json','config/payment-providers.example.json');copy('migrations/manifest.v1.json','migrations/manifest.v1.json');const migrationManifest=JSON.parse(readFileSync(join(root,'migrations/manifest.v1.json'),'utf8'));for(const ledger of migrationManifest.ledgers)for(const migration of ledger.migrations)copy(migration.path,migration.path);copy('deploy/archava.env.example','archava.env.example');copy('deploy/NATIVE_INSTALL.md','README.md')
 const catalog=JSON.parse(readFileSync(join(staging,'config/library-release.v1.json'),'utf8'));const artifacts=[]
 const walk=(dir)=>{for(const entry of readdirSync(join(staging,dir),{withFileTypes:true})){const path=dir?`${dir}/${entry.name}`:entry.name;if(entry.isDirectory())walk(path);else if(entry.isFile())artifacts.push({name:path,version:catalog.version,tarball:path,sha256:sha256(join(staging,path)),bytes:statSync(join(staging,path)).size})}}
 walk('');artifacts.sort((a,b)=>a.name.localeCompare(b.name,'en'))
 const profile={schemaVersion:1,distribution:'native',catalog:{id:catalog.id,version:catalog.version},runtimeVersion:/\[workspace.package\][\s\S]*?version\s*=\s*"([^"]+)"/.exec(readFileSync(join(root,'Cargo.toml'),'utf8'))?.[1],sourceSha256:built.source.sha256,artifacts}
 writeFileSync(join(staging,'release-profile.json'),JSON.stringify(profile,null,2)+'\n')
 execFileSync(join(staging,'bin/archava-ops'),['verify-release'],{cwd:staging,env:{...process.env,ARCHAVA_OPS_ROOT:staging},stdio:'inherit'})
 renameSync(staging,out);process.stdout.write(`PASS native distribution: ${out}\n`)
}finally{if(existsSync(staging))rmSync(staging,{recursive:true,force:true})}
