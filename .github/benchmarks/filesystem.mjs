import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {performance} from 'node:perf_hooks';
import {createHash} from 'node:crypto';
import {fileURLToPath, pathToFileURL} from 'node:url';
const self=fileURLToPath(import.meta.url);
const sha=data=>createHash('sha256').update(data).digest('hex');
if(process.argv[2]==='--worker'){
 const [root,source]=process.argv.slice(3);
 const {writeState,readState,lockState,releaseState}=await import(pathToFileURL(path.join(source,'platform/node/state-files.mjs')));
 const spans={},checks={};
 const measure=(key,fn)=>{const t=performance.now();const value=fn();spans[key]=performance.now()-t;return value;};
 for(const [name,count,size] of [['manySmall',1000,4096],['fewLarge',4,1024000]]){
  const directory=path.join(root,name),bytes=Buffer.alloc(size,97),expected=sha(bytes);
  measure(`${name}.create`,()=>{fs.mkdirSync(directory);for(let i=0;i<count;i++)fs.writeFileSync(path.join(directory,`${String(i).padStart(4,'0')}.dat`),bytes,{flag:'wx'});});
  const entries=measure(`${name}.metadata`,()=>fs.readdirSync(directory).sort().map(name=>({name,size:fs.lstatSync(path.join(directory,name)).size})));
  checks[`${name}.metadata`]=entries.length===count&&entries.every(e=>e.size===size);
  checks[`${name}.readHash`]=measure(`${name}.readHash`,()=>entries.every(e=>sha(fs.readFileSync(path.join(directory,e.name)))===expected));
  measure(`${name}.cleanup`,()=>fs.rmSync(directory,{recursive:true}));
  checks[`${name}.cleanup`]=!fs.existsSync(directory);
 }
 const stateDir=path.join(root,'.craft','state');fs.mkdirSync(stateDir,{recursive:true});
 const file=path.join(stateDir,'record.json'),limit=65536;
 writeState(file,{sequence:-1,payload:'a'.repeat(4096)},limit);
 let good=true;
 measure('state.100LockedReplacements',()=>{for(let sequence=0;sequence<100;sequence++){
  const ticket=lockState(file);if(!ticket)throw Error('Unexpected contention');
  try{writeState(file,{sequence,payload:'a'.repeat(4096)},limit);}finally{releaseState(ticket);}
 }});
 checks['state.replacement']=readState(file,limit)?.sequence===99;
 const holder=lockState(file);if(!holder)throw Error('Missing lock');
 try{measure('state.100RefusedLocks',()=>{for(let i=0;i<100;i++){const ticket=lockState(file);if(ticket){good=false;releaseState(ticket);}}});}finally{releaseState(holder);}
 checks['state.refusal']=good;
 checks['state.noTemporaryFiles']=JSON.stringify(fs.readdirSync(stateDir))===JSON.stringify(['record.json']);
 console.log(JSON.stringify({spans,checks,resourceUsage:process.resourceUsage()}));
 process.exitCode=Object.values(checks).every(Boolean)?0:1;
}else{
 const output=process.argv[2],rounds=Number(process.argv[3]??30),source=process.cwd();
 if(!output||fs.existsSync(output)||!Number.isInteger(rounds)||rounds<1)throw Error('Usage: node harness NEW_OUTPUT [positive rounds]');
 const git=args=>{const r=spawnSync('git',args,{encoding:'utf8'});if(r.status!==0)throw Error(r.stderr);return r.stdout.trim();};
 if(git(['status','--porcelain']))throw Error('Clean source required');
 const scratch=fs.mkdtempSync(path.join(os.tmpdir(),'craft-portable-filesystem-'));
 const evidence={schema:1,revision:git(['rev-parse','HEAD']),harnessSha256:sha(fs.readFileSync(self)),stateModuleSha256:sha(fs.readFileSync('platform/node/state-files.mjs')),startedAt:new Date().toISOString(),scratch,
 environment:{os:os.platform(),release:os.release(),arch:os.arch(),node:process.version,git:git(['--version']),cpu:os.cpus()[0]?.model,logicalCpus:os.cpus().length,memoryBytes:os.totalmem(),execution:'local shell tool; sandbox status supplied by operator',filesystemType:'unknown',diskCache:'uncontrolled'},
 protocol:{rounds,fixture:{manySmall:{files:1000,bytesPerFile:4096},fewLarge:{files:4,bytesPerFile:1024000},totalBytesPerShape:4096000},timing:'fresh worker each round; inner synchronous spans exclude launch/import; parent elapsed includes worker exit; first round separate',order:'manySmall then fewLarge then state; fixed order, no causal comparison',state:'real frozen platform primitives; 100 sequential locked replacements of ~4 KiB JSON, then 100 refusals under one held lock; batch times, not per-request tails',limits:'read/hash follows create and is warm; no fsync/crash durability, cross-process contention, large repository traversal or host scheduling measured; child resource usage includes both shapes/state/startup, not per-span',cleanup:'only newly created synthetic fixtures removed; raw evidence and retained state remain in private scratch'},samples:[]};
 for(let round=0;round<=rounds;round++){
  const root=path.join(scratch,`round-${round}`);fs.mkdirSync(root);
  const t=performance.now();const r=spawnSync(process.execPath,[self,'--worker',root,source],{encoding:'utf8',timeout:60000,maxBuffer:1024*1024});
  const sample={round,phase:round===0?'first':'repeated',wallMs:performance.now()-t,status:r.status,signal:r.signal};
  try{Object.assign(sample,JSON.parse(r.stdout));}catch{sample.invalidOutput=true;}
  sample.correct=r.status===0&&!r.error&&!r.stderr&&sample.checks&&Object.values(sample.checks).every(Boolean);
  if(!sample.correct)Object.assign(sample,{error:r.error?.message,stderr:r.stderr,stdout:r.stdout});
  evidence.samples.push(sample);fs.writeFileSync(output,JSON.stringify(evidence,null,2)+'\n');
 }
 const stats=v=>{const s=v.toSorted((a,b)=>a-b),n=s.length;return {n,median:(s[Math.floor((n-1)/2)]+s[Math.floor(n/2)])/2,p95:s[Math.ceil(n*.95)-1],min:s[0],max:s.at(-1)};};
 evidence.failures=evidence.samples.filter(s=>!s.correct).length;
 const repeated=evidence.samples.filter(s=>s.phase==='repeated'&&s.correct);
 evidence.summary=repeated.length?Object.fromEntries(['wallMs',...Object.keys(repeated[0].spans)].map(key=>[key,stats(repeated.map(s=>key==='wallMs'?s.wallMs:s.spans[key]))])):{};
 fs.writeFileSync(output,JSON.stringify(evidence,null,2)+'\n');console.log(JSON.stringify({summary:evidence.summary,failures:evidence.failures},null,2));
 if(evidence.failures)process.exitCode=1;
}
