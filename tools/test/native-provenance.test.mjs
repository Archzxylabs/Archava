import test from 'node:test'
import assert from 'node:assert/strict'
import { mkdtempSync,mkdirSync,writeFileSync,rmSync } from 'node:fs'
import { join } from 'node:path'
import { tmpdir } from 'node:os'
import { sourceManifest } from '../rust/provenance.mjs'
test('build provenance binds public inputs and ignores local secret/state files',()=>{
 const root=mkdtempSync(join(tmpdir(),'archava-provenance-test-'))
 try{
  mkdirSync(join(root,'crates/test/src'),{recursive:true});mkdirSync(join(root,'migrations'),{recursive:true});mkdirSync(join(root,'tools/rust'),{recursive:true});mkdirSync(join(root,'config'),{recursive:true})
  writeFileSync(join(root,'crates/test/src/lib.rs'),'pub fn test() {}\n');writeFileSync(join(root,'Cargo.toml'),'[workspace]\n');writeFileSync(join(root,'migrations/manifest.v1.json'),'{}');writeFileSync(join(root,'.env'),'PRIVATE=not-part-of-public-provenance');writeFileSync(join(root,'config/library-release.v1.json'),'{"version":"one"}')
  const before=sourceManifest(root);assert.equal(before.inputs.length,4);assert.ok(before.inputs.every(i=>!i.path.includes('.env')));assert.deepEqual(before,sourceManifest(root))
  writeFileSync(join(root,'.env'),'PRIVATE=changed-secret');assert.equal(sourceManifest(root).sha256,before.sha256)
  writeFileSync(join(root,'config/library-release.v1.json'),'{"version":"two"}');assert.notEqual(sourceManifest(root).sha256,before.sha256)
  writeFileSync(join(root,'crates/test/src/lib.rs'),'pub fn changed() {}\n');assert.notEqual(sourceManifest(root).sha256,before.sha256)
 }finally{rmSync(root,{recursive:true,force:true})}
})
