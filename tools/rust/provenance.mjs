/** Public build inputs and output digests. Never traverse local state or secret files. */
import { createHash } from 'node:crypto'
import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
export function sha256(path) { return createHash('sha256').update(readFileSync(path)).digest('hex') }
export function sourceManifest(root) {
 const inputs=[]
 const add=(path)=>{if(existsSync(join(root,path)))inputs.push({path,sha256:sha256(join(root,path))})}
 const walk=(dir)=>{for(const entry of readdirSync(join(root,dir),{withFileTypes:true})){
  if(entry.name.startsWith('.') && dir!=='.cargo')continue
  const path=`${dir}/${entry.name}`
  if(entry.isDirectory())walk(path)
  else if(entry.isFile() && (/\.(rs|toml|json|sql|css|woff2|png|svg|webp)$/.test(entry.name)))add(path)
 }}
 for(const dir of ['crates','migrations','static','config','.cargo'])if(existsSync(join(root,dir)))walk(dir)
 for(const path of ['Cargo.toml','Cargo.lock','rust-toolchain.toml','.tool-versions','tools/rust/build-slice.mjs','tools/rust/provenance.mjs'])add(path)
 inputs.sort((a,b)=>a.path.localeCompare(b.path,'en'))
 return {sha256:createHash('sha256').update(JSON.stringify(inputs)).digest('hex'),inputs}
}
