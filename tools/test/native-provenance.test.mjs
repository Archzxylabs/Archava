import test from 'node:test'
import assert from 'node:assert/strict'
import { mkdtempSync,mkdirSync,writeFileSync,rmSync } from 'node:fs'
import { join } from 'node:path'
import { tmpdir } from 'node:os'
import { sourceManifest } from '../rust/provenance.mjs'
import {domainAssets,DOMAIN_ASSETS} from '../rust/domain-assets.mjs'
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
test('changing an embedded SDK invalidates the serving build identity',()=>{
 const root=mkdtempSync(join(tmpdir(),'archava-sdk-provenance-test-'))
 try{
  mkdirSync(join(root,'static'),{recursive:true})
  writeFileSync(join(root,'static/archava.js'),'window.Archava = "original";\n')
  const built=sourceManifest(root)
  assert.ok(built.inputs.some(input=>input.path==='static/archava.js'))
  writeFileSync(join(root,'static/archava.js'),'window.Archava = "changed";\n')
  assert.notEqual(sourceManifest(root).sha256,built.sha256,'packaging must refuse an SDK changed after the serving build')
 }finally{rmSync(root,{recursive:true,force:true})}
})
test('missing or stale portable catalogues invalidate provenance before a serving build',()=>{
 const root=mkdtempSync(join(tmpdir(),'archava-domain-provenance-'))
 try{
  mkdirSync(join(root,'crates/domain'),{recursive:true});mkdirSync(join(root,'config'))
  writeFileSync(join(root,'crates/domain/Cargo.toml'),'[package]\nname="archava-domain"\n')
  for(const[path]of DOMAIN_ASSETS)writeFileSync(join(root,path),'{"schemaVersion":1}\n')
  assert.throws(()=>sourceManifest(root),/missing or stale/)
  domainAssets(root,{write:true});const before=sourceManifest(root)
  writeFileSync(join(root,DOMAIN_ASSETS[0][0]),'{"schemaVersion":2}\n')
  assert.throws(()=>sourceManifest(root),/missing or stale/)
  domainAssets(root,{write:true});assert.notEqual(sourceManifest(root).sha256,before.sha256)
 }finally{rmSync(root,{recursive:true,force:true})}
})
