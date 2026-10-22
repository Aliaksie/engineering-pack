import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {createHash} from 'node:crypto';

const source=path.resolve(process.argv[2]??''),output=path.resolve(process.argv[3]??'');
if(process.argv.length!==4||fs.existsSync(output))throw Error('Usage: node run.mjs FROZEN_CHECKOUT NEW_RESULTS_DIRECTORY');
const scripts=path.dirname(fileURLToPath(import.meta.url));
fs.mkdirSync(output,{recursive:true});
const hash=file=>createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const command=(exe,args,timeout=30000)=>{
 const r=spawnSync(exe,args,{cwd:source,encoding:'utf8',timeout,maxBuffer:4*1024*1024});
 return {status:r.status,signal:r.signal,stdout:r.stdout??'',stderr:r.stderr??'',error:r.error?.message};
};
const revision=command('git',['rev-parse','HEAD']);
if(revision.status!==0||revision.stdout.trim()!=='49fd3841e0bee5e76639c07078e3ccfd5d330b68')throw Error('Unexpected measured revision');
const keys=['RUNNER_OS','RUNNER_ARCH','RUNNER_NAME','RUNNER_ENVIRONMENT','ImageOS','ImageVersion','GITHUB_RUN_ID','GITHUB_RUN_ATTEMPT','GITHUB_SHA','GITHUB_REF'];
const evidence={startedAt:new Date().toISOString(),revision:revision.stdout.trim(),harnesses:Object.fromEntries(['run','components','filesystem','turn'].map(name=>[name,hash(path.join(scripts,`${name}.mjs`))])),environment:{node:process.version,os:os.platform(),release:os.release(),arch:os.arch(),cpu:os.cpus()[0]?.model,logicalCpus:os.cpus().length,totalMemoryBytes:os.totalmem(),runner:Object.fromEntries(keys.filter(k=>process.env[k]!==undefined).map(k=>[k,process.env[k]])),filesystem:{source:fs.statfsSync(source),temporary:fs.statfsSync(os.tmpdir())},shell:'workflow runner default; measured subprocesses use direct spawn without shell',cache:'OS cache uncontrolled; no application cache configured',git:command('git',['--version']).stdout.trim()},commands:[]};
const save=()=>fs.writeFileSync(path.join(output,'environment.json'),JSON.stringify(evidence,null,2)+'\n');
save();
// Invoke Node directly: no npm.cmd or platform-specific shell argument handling.
const commands=[['build',[path.join(source,'tooling/build.mjs'),'codex'],120000],...['components','filesystem','turn'].map(name=>[name,[path.join(scripts,`${name}.mjs`),path.join(output,`${name}.json`),'30'],600000])];
for(const [name,args,timeout] of commands){
 console.log(`Running ${name}`);
 const startedAt=new Date().toISOString(),r=command(process.execPath,args,timeout);
 evidence.commands.push({name,startedAt,...r});save();
 if(r.stdout)process.stdout.write(r.stdout);if(r.stderr)process.stderr.write(r.stderr);
 if(r.status!==0||r.error){process.exitCode=1;break;}
}
evidence.completedAt=new Date().toISOString();save();
