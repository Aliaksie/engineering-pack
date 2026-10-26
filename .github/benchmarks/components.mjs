import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {performance} from 'node:perf_hooks';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';

// Run unchanged on each native OS, from the same frozen source checkout.
const output = process.argv[2];
const rounds = Number(process.argv[3] ?? 30);
if (!output || fs.existsSync(output) || !Number.isInteger(rounds) || rounds < 1) throw Error('Usage: node harness.mjs NEW_OUTPUT [positive rounds]');
const source = process.cwd();
const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-portable-attribution-'));
const fixture = path.join(scratch, 'fixture');
const run = (command, args, cwd = source) => {
  const start = performance.now();
  const r = spawnSync(command, args, {cwd, encoding:'utf8', timeout:30000, maxBuffer:4*1024*1024});
  return {wallMs:performance.now()-start, status:r.status, signal:r.signal, stdout:r.stdout ?? '', stderr:r.stderr ?? '', error:r.error?.message};
};
const checked = (command,args,cwd) => {
  const r=run(command,args,cwd); if(r.status!==0 || r.error) throw Error(JSON.stringify(r)); return r.stdout.trim();
};
const revision=checked('git',['rev-parse','HEAD']);
if(checked('git',['status','--porcelain'])) throw Error('Tracked source must be clean');
const setupStart=performance.now();
fs.mkdirSync(fixture);
checked('git',['init','-q'],fixture);
// Local settings make fixture bytes/index independent of global CRLF and signing settings.
for(const [key,value] of [['core.autocrlf','false'],['core.hooksPath',path.join(scratch,'no-hooks')],['commit.gpgsign','false']]) checked('git',['config','--local',key,value],fixture);
const content='export const value = 1;\n'.repeat(20);
for(let i=0;i<100;i++) fs.writeFileSync(path.join(fixture,`file-${String(i).padStart(3,'0')}.mjs`),content);
checked('git',['add','.'],fixture);
checked('git',['-c','user.name=Fixture','-c','user.email=fixture@example.test','commit','-qm','fixture'],fixture);
const setupMs=performance.now()-setupStart;
const expectedDigest=createHash('sha256');
for(let i=0;i<100;i++){const name=`file-${String(i).padStart(3,'0')}.mjs`;expectedDigest.update(name).update('\0').update(content);}
const digest=expectedDigest.digest('hex');
const readScript="const fs=require('node:fs'),c=require('node:crypto');const h=c.createHash('sha256');let n=0,b=0;for(const f of fs.readdirSync('.').filter(f=>f.endsWith('.mjs')).sort()){const v=fs.readFileSync(f);h.update(f).update('\\0').update(v);n++;b+=v.length;}console.log(JSON.stringify({files:n,bytes:b,digest:h.digest('hex')}));";
const operations={
  nodeEmpty:{command:process.execPath,args:['-e',''],check:r=>r.stdout===''},
  gitVersion:{command:'git',args:['--version'],check:r=>r.stdout.startsWith('git version ')},
  gitIdentity:{command:'git',args:['rev-parse','--show-toplevel'],check:r=>{const actual=fs.statSync(path.resolve(r.stdout.trim()),{bigint:true}),expected=fs.statSync(fixture,{bigint:true});return actual.isDirectory()&&actual.ino!==0n&&actual.dev===expected.dev&&actual.ino===expected.ino;}},
  gitTracked:{command:'git',args:['ls-files','-z'],check:r=>r.stdout.split('\0').filter(Boolean).length===100},
  gitStatus:{command:'git',args:['status','--porcelain=v1','--untracked-files=all'],check:r=>r.stdout===''},
  readHash:{command:process.execPath,args:['-e',readScript],check:r=>{const v=JSON.parse(r.stdout);return v.files===100&&v.bytes===Buffer.byteLength(content)*100&&v.digest===digest;}},
  cliHelp:{command:process.execPath,args:[path.join(source,'tooling/craft.mjs'),'--help'],check:r=>Array.isArray(JSON.parse(r.stdout).commands)}
};
const evidence={schema:1,revision,startedAt:new Date().toISOString(),harnessSha256:createHash('sha256').update(fs.readFileSync(fileURLToPath(import.meta.url))).digest('hex'),scratch,
 environment:{os:os.platform(),release:os.release(),arch:os.arch(),node:process.version,git:checked('git',['--version']),cpu:os.cpus()[0]?.model,logicalCpus:os.cpus().length,memoryBytes:os.totalmem(),filesystemLocation:scratch,execution:'local shell tool; sandbox status supplied by operator',diskCache:'uncontrolled',compileCache:process.env.NODE_COMPILE_CACHE?'configured':'not configured',compileCacheDisabled:process.env.NODE_DISABLE_COMPILE_CACHE??null},
 fixture:{files:100,bytes:Buffer.byteLength(content)*100,digest,setupMs},protocol:{rounds,order:'rotate operations each round; first round separate from repeated summary',timing:'parent spawnSync through exit; includes launch, command, output, scheduling; no shell',limits:'process CPU/RSS and native host gates unknown; OS filesystem cache uncontrolled; Git config ambient except fixture autocrlf/hooks/signing; readHash includes Node launch; operation differences are not exclusive attribution',retention:'private scratch retained; no source or consumer state modified'},samples:[]};
const names=Object.keys(operations);
for(let round=0;round<=rounds;round++){
 for(let i=0;i<names.length;i++){
  const name=names[(i+round)%names.length],op=operations[name],r=run(op.command,op.args,fixture);
  let correct=false;try{correct=r.status===0&&!r.error&&r.stderr===''&&op.check(r);}catch{}
  evidence.samples.push({round,phase:round===0?'first':'repeated',name,wallMs:r.wallMs,status:r.status,signal:r.signal,correct,...(!correct?{stdout:r.stdout.slice(0,2000),stderr:r.stderr.slice(0,2000),error:r.error}:{})});
 }
 fs.writeFileSync(output,JSON.stringify(evidence,null,2)+'\n');
}
const stats=values=>{const s=values.toSorted((a,b)=>a-b),n=s.length;return {n,median:(s[Math.floor((n-1)/2)]+s[Math.floor(n/2)])/2,p95:s[Math.ceil(n*.95)-1],min:s[0],max:s.at(-1)};};
evidence.summary=Object.fromEntries(names.map(name=>[name,stats(evidence.samples.filter(s=>s.name===name&&s.phase==='repeated').map(s=>s.wallMs))]));
evidence.failures=evidence.samples.filter(s=>!s.correct).length;
fs.writeFileSync(output,JSON.stringify(evidence,null,2)+'\n');
console.log(JSON.stringify({revision,summary:evidence.summary,failures:evidence.failures,setupMs},null,2));
if(evidence.failures)process.exitCode=1;
