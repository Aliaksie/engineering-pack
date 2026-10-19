import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { parseVersion, incrementVersion, packVersion, synchronizeVersion } from '../tooling/version.mjs';
import { prepareRelease, publishReleaseRefs, releaseGuard } from '../tooling/release.mjs';

const git=(root,...args)=>execFileSync('git',args,{cwd:root,encoding:'utf8',stdio:['ignore','pipe','pipe']}).trim();
function fixture(t) {
  const directory=fs.mkdtempSync(path.join(os.tmpdir(),'pack-release-')), root=path.join(directory,'source'), remote=path.join(directory,'remote.git');
  t.after(()=>fs.rmSync(directory,{recursive:true,force:true}));
  fs.mkdirSync(root); git(directory,'init','--bare',remote); git(root,'init','--initial-branch=main');
  git(root,'config','user.name','Test'); git(root,'config','user.email','test@example.test');
  for (const name of ['craft-delivery','craft-engineering','craft-review','craft-stories']) {
    fs.mkdirSync(path.join(root,'packages',name),{recursive:true}); fs.writeFileSync(path.join(root,'packages',name,'capability.json'),'{"version":"0.0.2"}\n');
  }
  fs.mkdirSync(path.join(root,'platform/node'),{recursive:true});
  for(const file of ['package.json','platform/node/pack-version.json']) fs.writeFileSync(path.join(root,file),'{"version":"0.0.2"}\n');
  fs.writeFileSync(path.join(root,'.gitignore'),'dist/\n'); git(root,'add','.'); git(root,'commit','-m','Initial'); git(root,'remote','add','origin',remote); git(root,'push','origin','main');
  return {root,remote,source:git(root,'rev-parse','HEAD')};
}
test('stable SemVer parsing and increments reject ambiguous or unsafe versions',()=>{
  assert.deepEqual(parseVersion('0.0.2'),[0,0,2]);
  assert.equal(incrementVersion('1.2.3','patch'),'1.2.4'); assert.equal(incrementVersion('1.2.3','minor'),'1.3.0'); assert.equal(incrementVersion('1.2.3','major'),'2.0.0');
  for(const value of ['01.0.0','1.2','v1.2.3','1.2.3-rc','1.2.3+build','9007199254740992.0.0','1.2.3\n',null]) assert.throws(()=>parseVersion(value));
  assert.throws(()=>incrementVersion('1.2.3','other'));
});
test('canonical source version and projections agree; drift is rejected',t=>{
  const {root}=fixture(t); synchronizeVersion(root,'0.1.0'); assert.equal(packVersion(root),'0.1.0');
  synchronizeVersion(root,undefined,true);
  fs.writeFileSync(path.join(root,'platform/node/pack-version.json'),'{"version":"9.9.9"}'); assert.throws(()=>synchronizeVersion(root,undefined,true),/drift/);
  synchronizeVersion(process.cwd(),undefined,true);
});
test('current releases publish merged 0.0.2 without incrementing; tags and main agree',t=>{
  const {root,remote,source}=fixture(t), prepared=prepareRelease(root,source,'current');
  assert.equal(prepared.version,'0.0.2'); assert.equal(prepared.revision,source);
  fs.mkdirSync(path.join(root,'dist/distribution'),{recursive:true}); fs.writeFileSync(path.join(root,'dist/distribution/.engineering-distribution.json'),'{"version":"0.0.2"}');
  publishReleaseRefs(root,source,prepared.version,prepared.revision);
  assert.equal(git(root,'ls-remote',remote,'refs/tags/v0.0.2').split(/\s/)[0],source);
  assert.throws(()=>prepareRelease(root,source,'current'),/tag already exists/);
});
test('patch release commits only synchronized version metadata',t=>{
  const {root,source}=fixture(t), prepared=prepareRelease(root,source,'patch');
  assert.equal(prepared.version,'0.0.3'); assert.notEqual(prepared.revision,source);
  assert.equal(git(root,'status','--porcelain'),''); synchronizeVersion(root,'0.0.3',true);
});
test('wrong branch, revision, dirty checkout and advanced main block release',t=>{
  const {root,source}=fixture(t);
  assert.throws(()=>prepareRelease(root,'a'.repeat(40)),/main advanced/);
  git(root,'switch','-c','feature'); assert.throws(()=>releaseGuard(root,source),/requires main/); git(root,'switch','main');
  fs.writeFileSync(path.join(root,'private.txt'),'secret'); assert.throws(()=>prepareRelease(root,source),/clean checkout/); fs.unlinkSync(path.join(root,'private.txt'));
  git(root,'commit','--allow-empty','-m','Advanced'); git(root,'push','origin','main'); assert.throws(()=>releaseGuard(root,source),/main advanced/);
  assert.equal(git(root,'ls-remote','origin','refs/tags/*'),'');
});
test('unvalidated revision or inconsistent distribution cannot publish refs',t=>{
  const {root,source}=fixture(t), prepared=prepareRelease(root,source);
  assert.throws(()=>publishReleaseRefs(root,source,prepared.version,'a'.repeat(40)),/validated revision/);
  fs.mkdirSync(path.join(root,'dist/distribution'),{recursive:true}); fs.writeFileSync(path.join(root,'dist/distribution/.engineering-distribution.json'),'{"version":"0.0.1"}');
  assert.throws(()=>publishReleaseRefs(root,source,prepared.version,prepared.revision),/distribution version/);
  assert.equal(git(root,'ls-remote','origin','refs/tags/*'),'');
});

test('version synchronization validates every path before modifying projections',t=>{
  const {root}=fixture(t), file=path.join(root,'platform/node/pack-version.json');
  const outside=path.join(root,'outside.json'); fs.writeFileSync(outside,'{"version":"0.0.2"}');
  fs.unlinkSync(file); fs.symlinkSync(outside,file);
  assert.throws(()=>synchronizeVersion(root,'0.0.3'),/symlink/);
  assert.equal(packVersion(root),'0.0.2'); assert.equal(JSON.parse(fs.readFileSync(outside)).version,'0.0.2');
});

test('a rejected atomic push leaves remote main and tags unchanged',t=>{
  const {root,remote,source}=fixture(t), prepared=prepareRelease(root,source,'patch');
  fs.mkdirSync(path.join(root,'dist/distribution'),{recursive:true}); fs.writeFileSync(path.join(root,'dist/distribution/.engineering-distribution.json'),'{"version":"0.0.3"}');
  // Simulate remote policy rejecting publication after preparation succeeds.
  const hook=path.join(remote,'hooks/pre-receive');
  fs.writeFileSync(hook,`#!/bin/sh\nexit 1\n`,{mode:0o755});
  assert.throws(()=>publishReleaseRefs(root,source,prepared.version,prepared.revision));
  assert.equal(git(root,'ls-remote',remote,'refs/heads/main').split(/\s/)[0],source);
  assert.equal(git(root,'ls-remote',remote,'refs/tags/*'),'');
});
