/** Public compile-time data for standalone domain crates; root config stays canonical. */
import {createHash} from 'node:crypto'
import {existsSync,lstatSync,mkdirSync,readFileSync,realpathSync,writeFileSync} from 'node:fs'
import {dirname,relative,resolve} from 'node:path'
import {fileURLToPath} from 'node:url'

export const DOMAIN_ASSETS=Object.freeze([
 ['config/operator-actions.v1.json','crates/domain/embedded/operator-actions.v1.json'],
 ['config/accounting-currencies.v1.json','crates/domain/embedded/accounting-currencies.v1.json'],
 ['config/modules.v1.json','crates/domain/embedded/modules.v1.json'],
 ['config/refusal-reasons.v1.json','crates/domain/embedded/refusal-reasons.v1.json'],
 ['config/tax-templates.v1.json','crates/domain/embedded/tax-templates.v1.json'],
])
const digest=bytes=>createHash('sha256').update(bytes).digest('hex')
function inside(root,path){const part=relative(realpathSync(root),realpathSync(path));if(part.startsWith('..')||part.startsWith('/'))throw Error('Domain assets must remain inside the selected repository')}
export function domainAssets(root,{write=false}={}){
 root=resolve(root);const result=[]
 for(const[canonical,embedded]of DOMAIN_ASSETS){
  const source=resolve(root,canonical),destination=resolve(root,embedded)
  if(!existsSync(source)||!lstatSync(source).isFile()||lstatSync(source).isSymbolicLink())throw Error(`Missing regular canonical asset: ${canonical}`)
  inside(root,source);const bytes=readFileSync(source);JSON.parse(bytes.toString('utf8'))
  const parent=dirname(destination)
  const parentStat=lstatSync(parent,{throwIfNoEntry:false}),destinationStat=lstatSync(destination,{throwIfNoEntry:false})
  if(parentStat){if(parentStat.isSymbolicLink())throw Error(`Embedded asset directory cannot be a symlink: ${embedded}`);inside(root,parent)}
  if(destinationStat&&(!destinationStat.isFile()||destinationStat.isSymbolicLink()))throw Error(`Embedded asset must be a regular file: ${embedded}`)
  if(write){
   // Check the existing crate parent before creating this task's generated directory.
   inside(root,dirname(parent));mkdirSync(parent,{recursive:true});writeFileSync(destination,bytes)
  }
  if(!existsSync(destination)||!readFileSync(destination).equals(bytes))throw Error(`Embedded domain asset is missing or stale: ${embedded}. Run node tools/rust/domain-assets.mjs --write`)
  result.push({canonical,embedded,sha256:digest(bytes),bytes:bytes.length})
 }
 return result
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const args=process.argv.slice(2)
 if(args.length>1||args.length===1&&!['--check','--write','--help'].includes(args[0])){process.stderr.write('Use --check or --write.\n');process.exitCode=2}
 else if(args[0]==='--help')process.stdout.write('node tools/rust/domain-assets.mjs [--check|--write]\nCheck or synchronize only the five public canonical catalogues into the standalone domain crate. No private environment or other configuration is copied.\n')
 else{try{const assets=domainAssets(resolve(dirname(fileURLToPath(import.meta.url)),'../..'),{write:args[0]==='--write'});process.stdout.write(`PASS public domain assets: ${assets.length} exact snapshots\n`)}catch(e){process.stderr.write(`${e.message}\n`);process.exitCode=1}}
}
