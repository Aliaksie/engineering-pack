import test from 'node:test';
import assert from 'node:assert/strict';
import {assessReports, formatSummary} from '../.github/benchmarks/report.mjs';
const revision='a'.repeat(40);
function reports() {
  const base=()=>({revision,failures:0,protocol:{rounds:1},summary:{operation:{n:1,median:100,p95:120}},samples:[{correct:true}]});
  return {components:base(),filesystem:base(),turn:{...base(),exportsUnchanged:true,reportedDrops:0,samples:[{wireMatches:true,stateMatches:true,telemetryMatches:true,dropped:0}]}};
}
test('timing variance and reported telemetry loss remain visible diagnostics',()=>{
  const r=reports();r.turn.summary.operation.median=100000;r.turn.summary.operation.p95=200000;
  r.turn.reportedDrops=2;r.turn.samples[0].telemetryMatches=false;r.turn.samples[0].dropped=2;
  const result=assessReports(r,revision);
  assert.deepEqual(result.failures,[]);assert.match(result.diagnostics[0],/2 reported drops in 1 turns/);
  assert.match(formatSummary(result,r,revision),/100000.0/);
});
for(const [name,mutate,expected] of [
  ['wire mismatch',r=>r.turn.samples[0].wireMatches=false,/wire output/],
  ['state mismatch',r=>r.turn.samples[0].stateMatches=false,/task state/],
  ['process failure',r=>r.turn.failures=1,/process or assertion/],
  ['component failure',r=>r.components.samples[0].correct=false,/failed sample/],
  ['modified export',r=>r.turn.exportsUnchanged=false,/exported source/],
  ['wrong revision',r=>r.components.revision='b'.repeat(40),/unexpected measured revision/],
  ['partial measurement',r=>r.filesystem.summary.operation.n=0,/incomplete/],
  ['missing artifact',r=>delete r.turn,/missing or unreadable/]
])test(`${name} fails assessment`,()=>{const r=reports();mutate(r);assert.match(assessReports(r,revision).failures.join('\n'),expected);});
