import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawnSync,spawn} from 'node:child_process';
import {performance} from 'node:perf_hooks';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
const output=process.argv[2],rounds=Number(process.argv[3]??30);
if(!output||fs.existsSync(output)||!Number.isInteger(rounds)||rounds<1)throw Error('Usage: node harness NEW_OUTPUT [positive rounds]');
const source=process.cwd(),scratch=fs.mkdtempSync(path.join(os.tmpdir(),'craft-portable-turn-'));
const hash=b=>createHash('sha256').update(b).digest('hex');
const run=(cmd,args,cwd=source)=>{const r=spawnSync(cmd,args,{cwd,encoding:'utf8',timeout:30000,maxBuffer:4*1024*1024});if(r.status!==0)throw Error(r.stderr||r.error?.message);return r.stdout.trim();};
if(run('git',['status','--porcelain']))throw Error('Clean tracked source required');
const packages=['craft-delivery','craft-engineering','craft-review','craft-stories'];
const inventory=root=>{const result={};const walk=d=>{for(const e of fs.readdirSync(d,{withFileTypes:true}).sort((a,b)=>a.name.localeCompare(b.name))){const f=path.join(d,e.name);if(e.isDirectory())walk(f);else if(e.isFile())result[path.relative(root,f).split(path.sep).join('/')]=hash(fs.readFileSync(f));else throw Error('Unexpected export entry');}};walk(root);return result;};
for(const pkg of packages)fs.cpSync(path.join(source,'dist/codex/packages',pkg),path.join(scratch,'exports',pkg),{recursive:true});
const hashes=inventory(path.join(scratch,'exports'));
const root=path.join(scratch,'fixture');fs.mkdirSync(root);
const git=(...args)=>run('git',args,root);
git('init','-q');for(const [k,v] of [['core.autocrlf','false'],['core.hooksPath',path.join(scratch,'no-hooks')],['commit.gpgsign','false']])git('config','--local',k,v);
for(let i=0;i<100;i++)fs.writeFileSync(path.join(root,`file-${i}.mjs`),'export const value=1;\n'.repeat(20));
git('add','.');git('-c','user.name=Fixture','-c','user.email=fixture@example.test','commit','-qm','fixture');
run(process.execPath,[path.join(source,'cli/main.mjs'),'task-start','--root',root,'--session','bench','--task','task','--goal','Fixed synthetic coding turn','--json']);
const seed=path.join(scratch,'seed');fs.cpSync(path.join(root,'.craft'),seed,{recursive:true});
const manifests=Object.fromEntries(packages.map(p=>[p,JSON.parse(fs.readFileSync(path.join(scratch,'exports',p,'platform-hooks/hooks.json')))]));
const workloads=Object.fromEntries(['nativeReads','shellReads'].map(name=>{
 const steps=[{event:'UserPromptSubmit',payload:{cwd:root,session_id:'bench',prompt:'Inspect and update file-0.mjs, then verify'}}];
 const reads=name==='nativeReads'?[['Read',{file_path:path.join(root,'file-0.mjs')}],['Read',{file_path:path.join(root,'file-1.mjs')}] ]:[['Bash',{command:'cat file-0.mjs'}],['Bash',{command:'cat file-1.mjs'}]];
 for(const [tool_name,tool_input] of [...reads,['Edit',{file_path:path.join(root,'file-0.mjs'),old_string:'1',new_string:'2'}],['Bash',{command:'git status --short'}]])for(const event of ['PreToolUse','PostToolUse'])steps.push({event,payload:{cwd:root,session_id:'bench',tool_name,tool_input}});
 steps.push({event:'Stop',payload:{cwd:root,session_id:'bench'}});
 for(const s of steps)s.wrappers=packages.flatMap(pkg=>(manifests[pkg].hooks[s.event]??[]).filter(e=>!e.matcher||new RegExp(e.matcher).test(s.payload.tool_name??'')).flatMap(e=>e.hooks.map(h=>{const m=/^"\$\{PLUGIN_ROOT\}\/(bin\/craft)" (hook .+)$/.exec(h.command);if(!m)throw Error(h.command);return {pkg,file:m[1],args:m[2].split(' ')};})));
 return [name,steps];
}));
const normalize=events=>events.map(e=>{const v=structuredClone(e);for(const k of ['timestamp','sessionId','operationId','traceId','parentOperationId','durationMs'])delete v[k];return v;}).sort((a,b)=>JSON.stringify(a).localeCompare(JSON.stringify(b)));
const evidence={revision:run('git',['rev-parse','HEAD']),harnessSha256:hash(fs.readFileSync(fileURLToPath(import.meta.url))),startedAt:new Date().toISOString(),scratch,hashes,environment:{node:process.version,os:os.platform(),release:os.release(),arch:os.arch(),cpu:os.cpus()[0]?.model,git:run('git',['--version']),diskCache:'uncontrolled',execution:'local shell tool; sandbox status supplied by operator'},protocol:{rounds,timing:'sequential lifecycle gates, wrappers concurrently spawned per gate; parent elapsed includes child close; seed reset outside timer',order:'first round separate, alternate workload order each round',limits:'synthetic manual dispatch, all four packages explicitly selected; no actual tools/model/host scheduling; shell payloads are not executed, including on Windows',correctness:'process wire equality across repetitions, unchanged task binding/record, normalized retained telemetry compared to first sample; telemetry loss is reported separately; not crash/timeout/concurrent-mutation equivalence'},workloads,samples:[]};
const reference={};
for(let round=0;round<=rounds;round++)for(const name of round%2?['shellReads','nativeReads']:['nativeReads','shellReads']){
 const resetStart=performance.now();fs.rmSync(path.join(root,'.craft'),{recursive:true,force:true});fs.cpSync(seed,path.join(root,'.craft'),{recursive:true});const resetMs=performance.now()-resetStart;
 const gates=[],wrappers=[],start=performance.now();
 for(const [stepIndex,s] of workloads[name].entries()){
  const gateStart=performance.now();
  const receipts=await Promise.all(s.wrappers.map(w=>new Promise(resolve=>{
   const t=performance.now(),child=spawn(path.join(scratch,'exports',w.pkg,w.file),w.args,{cwd:root,stdio:['pipe','pipe','pipe']});let stdout='',stderr='',settled=false;
   const finish=(status,error)=>{if(settled)return;settled=true;clearTimeout(timer);resolve({...w,stepIndex,status,stdout,stderr,error,wallMs:performance.now()-t});};
   const timer=setTimeout(()=>{child.kill();finish(null,'30-second timeout');},30000);
   child.stdout.on('data',b=>stdout+=b);child.stderr.on('data',b=>stderr+=b);child.once('error',e=>finish(null,e.message));child.once('close',s=>finish(s));child.stdin.on('error',e=>finish(null,e.message));child.stdin.end(JSON.stringify(s.payload));
  })));
  wrappers.push(...receipts);gates.push({stepIndex,event:s.event,wallMs:performance.now()-gateStart,processes:receipts.length});
 }
 const wallMs=performance.now()-start,events=[],state={};
 const walk=d=>{for(const e of fs.readdirSync(d,{withFileTypes:true})){const f=path.join(d,e.name);if(e.isDirectory())walk(f);else if(e.name==='telemetry.jsonl'){for(const line of fs.readFileSync(f,'utf8').split('\n').filter(Boolean))events.push(JSON.parse(line));}else if(e.name.endsWith('.json')){const v=JSON.parse(fs.readFileSync(f));if(v.taskRecord)state.task={record:v.taskRecord,attempts:v.attempts.length};if(v.formatVersion===1&&typeof v.task==='string')state.binding={task:v.task,checkout:v.checkout,repositories:v.repositories};}}};walk(path.join(root,'.craft'));
 const wire=wrappers.map(({wallMs,...w})=>w),normalized=normalize(events);
 reference[name]??={wire,state,events:normalized};
 const sample={round,name,phase:round===0?'first':'repeated',wallMs,resetMs,gates,wrappers,state,events:normalized,dropped:events.filter(e=>e.reasonCode==='TELEMETRY_EVENTS_DROPPED').reduce((n,e)=>n+(e.attributes?.droppedEvents??0),0),wireMatches:JSON.stringify(wire)===JSON.stringify(reference[name].wire),stateMatches:JSON.stringify(state)===JSON.stringify(reference[name].state),telemetryMatches:JSON.stringify(normalized)===JSON.stringify(reference[name].events)};
 evidence.samples.push(sample);fs.writeFileSync(output,JSON.stringify(evidence,null,2)+'\n');
 if(round%5===0)console.log(`${round}/${rounds} ${name}: ${wallMs.toFixed(1)} ms`);
}
const stats=v=>{const s=v.toSorted((a,b)=>a-b),n=s.length;return {n,median:(s[Math.floor((n-1)/2)]+s[Math.floor(n/2)])/2,p95:s[Math.ceil(n*.95)-1],min:s[0],max:s.at(-1)};};
evidence.summary=Object.fromEntries(Object.keys(workloads).map(name=>[name,stats(evidence.samples.filter(s=>s.name===name&&s.phase==='repeated').map(s=>s.wallMs))]));
evidence.failures=evidence.samples.flatMap(s=>s.wrappers).filter(w=>w.status!==0||w.stderr||w.error).length;
evidence.mismatches=evidence.samples.filter(s=>!s.wireMatches||!s.stateMatches||!s.telemetryMatches).length;
evidence.reportedDrops=evidence.samples.reduce((n,s)=>n+s.dropped,0);
evidence.exportsUnchanged=JSON.stringify(inventory(path.join(scratch,'exports')))===JSON.stringify(hashes);
fs.writeFileSync(output,JSON.stringify(evidence,null,2)+'\n');console.log(JSON.stringify({summary:evidence.summary,failures:evidence.failures,mismatches:evidence.mismatches,reportedDrops:evidence.reportedDrops,exportsUnchanged:evidence.exportsUnchanged},null,2));
if(evidence.failures||!evidence.exportsUnchanged)process.exitCode=1;
