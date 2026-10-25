import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { nativeControl, stateFile } from '../platform/index.mjs';
import { digest } from '../application/engine-client.mjs';
import { withTaskAdmission } from '../hosts/native-control.mjs';
import { run } from '../cli/main.mjs';
function fixture(t) {
  const root=fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(),'craft-native-control-')));
  t.after(()=>fs.rmSync(root,{recursive:true,force:true}));
  return {root,session:'s',task:'t',host:'claude',coordinator:'main',mode:'managed'};
}
const call=(base,phase,input={})=>nativeControl({...base,phase,...input});
const file=(base,tree)=>stateFile(base.root,`native-control-${digest([base.host,tree]).slice(0,24)}.json`,true,base.session);
test('native journal fences live owners, duplicate keys, stale revisions and superseded tickets',t=>{
  const base=fixture(t), opened=call(base,'open',{pid:process.pid});
  const ticket=opened.ticket;
  const request={key:'a'.repeat(64),source:null};
  assert.throws(()=>call(base,'open',{pid:process.pid}),/already has an owner/);
  const begun=call(base,'begin',{ticket,expected:opened.revision,input:request,kind:'worker',nativeId:null});
  assert.throws(()=>call(base,'begin',{ticket,expected:begun.revision,input:request,kind:'worker'}),/already attempted/);
  assert.throws(()=>call(base,'persist',{ticket,expected:opened.revision,journal:opened.journal}),/journal changed/);
  const changed=structuredClone(begun.journal); changed.operations[0].source='b'.repeat(64);
  assert.throws(()=>call(base,'persist',{ticket,expected:begun.revision,journal:changed}),/identity changed/);
  assert.equal(JSON.parse(fs.readFileSync(file(base,opened.context.tree))).operations.length,1);
  call(base,'close',{ticket});
  const next=call(base,'open',{pid:process.pid});
  assert.notEqual(next.ticket.lease,ticket.lease);
  assert.equal(next.journal.operations[0].phase,'intent');
  assert.throws(()=>call(base,'close',{ticket}),/ownership changed/);
  assert.throws(()=>call(base,'open',{pid:process.pid}),/already has an owner/);
  call(base,'close',{ticket:next.ticket});
});
test('malformed and legacy locks remain busy, and corrupt journals release only their own lease',t=>{
  const base=fixture(t), opened=call(base,'open',{pid:process.pid});call(base,'close',{ticket:opened.ticket});
  const journal=file(base,opened.context.tree), lock=`${journal}.lock`;
  for(const bytes of ['', '{}', `{"owner":"craft-native-lease","formatVersion":1,"pid":${process.pid}.0,"lease":"${'a'.repeat(64)}"}`, JSON.stringify({owner:'craft-native-lease',formatVersion:1,pid:0,lease:'a'.repeat(64)})]) {
    fs.writeFileSync(lock,bytes);
    assert.throws(()=>call(base,'open',{pid:process.pid}),/already has an owner/);
    assert.equal(fs.readFileSync(lock,'utf8'),bytes);fs.unlinkSync(lock);
  }
  fs.writeFileSync(journal,JSON.stringify({...opened.journal,operations:[{key:'bad'}]}));
  assert.throws(()=>call(base,'open',{pid:process.pid}),/invalid bounded native/);
  assert.equal(fs.existsSync(lock),false);
});
test('admission releases its binding fence on callback failure',async t=>{
  const base=fixture(t);
  run(['task-start','--root',base.root,'--session',base.session,'--task',base.task,'--goal','Inspect explicit scope']);
  await assert.rejects(withTaskAdmission(base,async()=>{throw new Error('callback failed');}),/callback failed/);
  run(['task-start','--root',base.root,'--session',base.session,'--task','other','--goal','Continue scope']);
  await assert.rejects(withTaskAdmission(base,async()=>{}),/active task binding/);
});
test('dead host ownership is reclaimed by journal open and ordinary task-start without replay',async t=>{
  const base=fixture(t);
  // A non-Git task still has a bound checkout identity and must respect admission.
  run(['task-start','--root',base.root,'--session',base.session,'--task',base.task,'--goal','Inspect explicit scope']);
  const module=new URL('../platform/index.mjs',import.meta.url).href;
  const script=`import {nativeControl} from ${JSON.stringify(module)};
    const base=${JSON.stringify(base)};
    const call=(phase,input={})=>nativeControl({...base,phase,...input});
    const opened=call('open',{pid:process.pid});
    call('begin',{ticket:opened.ticket,expected:opened.revision,input:{key:'a'.repeat(64)},kind:'worker'});
    const admission=call('admission-acquire',{pid:process.pid});
    process.stdout.write(JSON.stringify({ticket:opened.ticket,admission})+'\\n');
    setInterval(()=>{},1000);`;
  const child=spawn(process.execPath,['--input-type=module','-e',script],{stdio:['ignore','pipe','pipe'],env:process.env});
  t.after(()=>child.kill());
  let errors='';child.stderr.on('data',chunk=>errors+=chunk);
  const observed=await new Promise((resolve,reject)=>{
    const timer=setTimeout(()=>{child.kill();reject(new Error(`child owner timeout: ${errors}`));},15000);
    child.once('error',error=>{clearTimeout(timer);reject(error);});
    child.once('exit',()=>{clearTimeout(timer);reject(new Error(`child exited before ownership: ${errors}`));});
    let output='';child.stdout.on('data',chunk=>{output+=chunk;if(output.includes('\n')){clearTimeout(timer);resolve(JSON.parse(output));}});
  });
  assert.throws(()=>call(base,'open',{pid:process.pid}),/already has an owner/);
  assert.throws(()=>run(['task-start','--root',base.root,'--session',base.session,'--task','other','--goal','New scope']),/writer is busy/);
  const exited=once(child,'exit');child.kill('SIGKILL');await exited;
  const recovered=call(base,'open',{pid:process.pid});
  assert.equal(recovered.journal.operations.length,1);assert.equal(recovered.journal.operations[0].phase,'intent');
  assert.throws(()=>call(base,'close',{ticket:observed.ticket}),/ownership changed/);
  call(base,'close',{ticket:recovered.ticket});
  run(['task-start','--root',base.root,'--session',base.session,'--task','other','--goal','New scope']);
  assert.throws(()=>call(base,'admission-check',{ticket:observed.admission}),/unavailable|ownership/);
});

test('native capability support matches the shared export facts for every adapter',()=>{
  const table=JSON.parse(fs.readFileSync(new URL('../hosts/capabilities.json',import.meta.url)));
  for(const [host,expected] of Object.entries(table)) assert.deepEqual(nativeControl({phase:'support',host}),expected);
  assert.throws(()=>nativeControl({phase:'support',host:'missing'}),/unknown host adapter/);
});
