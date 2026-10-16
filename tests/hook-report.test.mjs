import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { policyReport, localHookReport } from '../scripts/hook-report.mjs';

test('hook report exposes false and missed blocks without treating its corpus as a success gate', () => {
  const cases = [
    { id: 'read', command: 'read', expected: 'allow' },
    { id: 'write', command: 'write', expected: 'deny' }
  ];
  const result = policyReport(() => ({ decision: 'deny' }), cases);
  assert.equal(result.falseBlocks, 1);
  assert.equal(result.missedBlocks, 0);
  const permissive = policyReport(() => ({ decision: 'allow' }), cases);
  assert.equal(permissive.missedBlocks, 1);
  assert.match(result.limitation, /not a sampled production/);
});

test('local measurement observes generated allow/deny processes and bounds its workload', () => {
  assert.throws(() => localHookReport(path.resolve('.'), 0), /iterations/);
  const report = localHookReport(path.resolve('.'), 2);
  assert.equal(report.latency.samples.length, 2);
  assert.deepEqual(report.latency.samples.map((sample) => sample.decision), ['allow', 'deny']);
  assert.ok(report.latency.samples.every((sample) => sample.milliseconds > 0));
  assert.equal(report.platform, process.platform);
  assert.match(report.latency.limitation, /no host dispatch/);
});

test('measurement refuses exports that do not match the source being reported', async (t) => {
  const fs = await import('node:fs');
  const os = await import('node:os');
  const directory = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'stale hook report ')));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  for (const folder of ['runtime', 'adapters/shared', 'packages/craft-review/hooks', 'dist/claude/plugins/craft-review']) {
    fs.cpSync(path.resolve(folder), path.join(directory, folder), { recursive: true });
  }
  fs.appendFileSync(path.join(directory, 'runtime/project.mjs'), '\n// Changed after export.\n');
  assert.throws(() => localHookReport(directory, 1), /exports are stale/);
});
