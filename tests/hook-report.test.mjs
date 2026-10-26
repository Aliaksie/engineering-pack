import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import fs from 'node:fs';
import os from 'node:os';
import { policyReport, localHookReport, hookPolicy, readOnlyHook } from '../tooling/hook-report.mjs';

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
  assert.equal(permissive.unexpectedDecisions, 0);
  assert.match(result.limitation, /not a sampled production/);
});

test('policy report counts unexpected decisions separately from false and missed blocks', () => {
  for (const decision of [undefined, null, 'ask']) {
    const report = policyReport(() => ({ decision }), [{ id: 'read', command: 'read', expected: 'allow' }, { id: 'write', command: 'write', expected: 'deny' }]);
    assert.equal(report.unexpectedDecisions, report.cases.length);
    assert.equal(report.falseBlocks, 0);
    assert.equal(report.missedBlocks, 0);
  }
});

test('read-only review catches literal shell scripts and active command substitutions', t => {
  const project = config => {
    const directory = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'read-only review ')));
    t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
    fs.mkdirSync(path.join(directory, '.craft'));
    fs.writeFileSync(path.join(directory, '.craft/project.json'), JSON.stringify({ formatVersion: 1, ...config }));
    return directory;
  };
  const hook = readOnlyHook(path.resolve('.'));
  const policy = hookPolicy(hook, project({ review: { readOnly: true } }));
  const decision = command => policy({ command }).decision;
  for (const command of [
    'sh -c "git push"', "bash -lc 'git status; git push'", '/bin/sh -c "gh pr comment 1 --body review"',
    'echo "$(git push)"', 'echo `git push`', 'echo "$(echo $(git push))"',
    'echo "$(echo \'safe)\'; git push)"', 'sh -c \'echo "$(git push)"\''
  ]) assert.equal(decision(command), 'deny', command);
  for (const command of [
    'sh -c "git status"', 'echo "$(git status)"', "echo '$(git push)'", "echo '`git push`'",
    'echo "\\$(git push)"', 'echo "git push"', 'echo "$(echo \'git push\')"'
  ]) assert.equal(decision(command), 'allow', command);
  assert.equal(policyReport(policy).missedBlocks, 0);
  assert.equal(hookPolicy(hook, project({}))({ command: 'sh -c "git push"' }).decision, 'allow');
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
  for (const folder of ['packages/craft-review', 'dist/claude/plugins/craft-review']) {
    fs.cpSync(path.resolve(folder), path.join(directory, folder), { recursive: true });
  }
  fs.appendFileSync(path.join(directory, 'packages/craft-review/hooks/read-only.mjs'), '\n// Changed after export.\n');
  assert.throws(() => localHookReport(directory, 1), /exports are stale/);
});
