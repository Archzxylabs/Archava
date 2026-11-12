import test from 'node:test'
import assert from 'node:assert/strict'
import {existsSync,mkdtempSync,mkdirSync,writeFileSync,readFileSync,readdirSync,rmSync,symlinkSync} from 'node:fs'
import {tmpdir} from 'node:os'
import {join} from 'node:path'
import {DOMAIN_ASSETS,domainAssets} from '../rust/domain-assets.mjs'
function fixture(){const root=mkdtempSync(join(tmpdir(),'archava-domain-assets-'));mkdirSync(join(root,'config'));mkdirSync(join(root,'crates/domain'),{recursive:true});for(const[path]of DOMAIN_ASSETS)writeFileSync(join(root,path),JSON.stringify({schemaVersion:1,name:path})+'\n');return root}
test('standalone domain embeds preserve exact canonical bytes and declared public inputs only',()=>{
 const root=fixture()
 try{writeFileSync(join(root,'config/private.env'),'DO_NOT_COPY=synthetic\n');const result=domainAssets(root,{write:true});assert.equal(result.length,2);for(const r of result)assert.deepEqual(readFileSync(join(root,r.embedded)),readFileSync(join(root,r.canonical)));assert.equal(readdirSync(join(root,'crates/domain/embedded')).length,2);assert.deepEqual(domainAssets(root),result)}finally{rmSync(root,{recursive:true,force:true})}
})
test('missing or stale public data fails instead of packaging an old catalogue',()=>{
 const root=fixture()
 try{assert.throws(()=>domainAssets(root),/missing or stale/);assert.equal(existsSync(join(root,'crates/domain/embedded')),false);domainAssets(root,{write:true});writeFileSync(join(root,DOMAIN_ASSETS[0][0]),'{"schemaVersion":2}\n');assert.throws(()=>domainAssets(root),/missing or stale/);domainAssets(root,{write:true});assert.equal(domainAssets(root).length,2)}finally{rmSync(root,{recursive:true,force:true})}
})
test('an embedded data symlink cannot redirect a catalogue synchronization',()=>{
 const root=fixture(),outside=mkdtempSync(join(tmpdir(),'archava-domain-assets-outside-'))
 try{const protectedFile=join(outside,'preserve.json');writeFileSync(protectedFile,'preserve\n');mkdirSync(join(root,'crates/domain/embedded'));symlinkSync(protectedFile,join(root,DOMAIN_ASSETS[0][1]));assert.throws(()=>domainAssets(root,{write:true}),/regular file/);assert.equal(readFileSync(protectedFile,'utf8'),'preserve\n')}finally{rmSync(root,{recursive:true,force:true});rmSync(outside,{recursive:true,force:true})}
})
test('a generated directory symlink cannot escape the declared crate',()=>{
 const root=fixture(),outside=mkdtempSync(join(tmpdir(),'archava-domain-assets-dir-'))
 try{symlinkSync(outside,join(root,'crates/domain/embedded'));assert.throws(()=>domainAssets(root,{write:true}),/cannot be a symlink/);assert.deepEqual(readdirSync(outside),[])}finally{rmSync(root,{recursive:true,force:true});rmSync(outside,{recursive:true,force:true})}
})
test('a dangling embedded symlink cannot create a file outside the declared crate',()=>{
 const root=fixture(),outside=mkdtempSync(join(tmpdir(),'archava-domain-assets-dangling-'))
 try{mkdirSync(join(root,'crates/domain/embedded'));const destination=join(outside,'must-not-create.json');symlinkSync(destination,join(root,DOMAIN_ASSETS[0][1]));assert.throws(()=>domainAssets(root,{write:true}),/regular file/);assert.equal(existsSync(destination),false)}finally{rmSync(root,{recursive:true,force:true});rmSync(outside,{recursive:true,force:true})}
})
