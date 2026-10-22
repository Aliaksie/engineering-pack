import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { run } from '../cli/main.mjs';
import { groundedContext } from '../application/session/grounding.mjs';
import { engineeringContractDigest } from '../engine/session/acceptance.mjs';

const fixture = new URL('../examples/evals/developer-acceptance/fixture/', import.meta.url);
const at = '2026-10-22T00:00:00Z';
function setup(t, structured = true) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-developer-')));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.cpSync(fixture, root, { recursive: true });
  const git = (...args) => {
    const result = spawnSync('git', ['-C', root, '-c', 'core.hooksPath=.no-hooks', '-c', 'commit.gpgsign=false', '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test', ...args], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    return result.stdout.trim();
  };
  git('init', '-q'); git('add', '.'); git('commit', '-qm', 'fixture');
  const checkEnv = { ...process.env };
  delete checkEnv.NODE_TEST_CONTEXT;
  const check = id => spawnSync(process.execPath, id === 'syntax'
    ? ['--check', 'src/receipt.mjs'] : ['--test', `--test-name-pattern=${id}`, 'acceptance.test.mjs'],
  { cwd: root, encoding: 'utf8', env: checkEnv });
  const command = (name, ...args) => run([name, '--root', root, '--session', 'fixture-session', ...args]);
  const fix = () => {
    const file = path.join(root, 'src/receipt.mjs');
    fs.writeFileSync(file, fs.readFileSync(file, 'utf8').replace('      normalizeKey(key);', '      key = normalizeKey(key);'));
  };
  if (!structured) return { root, check, fix };
  const source = (id, kind, reference) => ({ id, kind, reference,
    identity: createHash('sha256').update(fs.readFileSync(path.join(root, reference))).digest('hex'),
    observedAt: at, scope: ['src'], provenance: 'observed', status: 'accepted' });
  const input = { scope: ['src'], sources: [source('policy', 'instruction', 'AGENTS.md'),
    source('checks', 'check', 'acceptance.test.mjs'), source('reuse', 'code', 'src/normalize-key.mjs')],
  constraints: [{ id: 'dependencies', description: 'No dependencies; reuse normalizeKey without changing its contract',
    sourceIds: ['policy', 'reuse'], authority: 'project', status: 'accepted', scope: ['src'], required: true, conflicts: [] }],
  checks: ['retry', 'invalid'].map(id => ({ id, description: `node --test --test-name-pattern=${id} acceptance.test.mjs`, sourceIds: ['checks'], scope: ['src'] })) };
  const context = groundedContext(root, input).context;
  const contract = { formatVersion: 1, intent: 'Canonical receipt identity with invalid-input preservation',
    exclusions: ['Dependencies, commits and remote actions in model runs'], context,
    criteria: ['retry', 'invalid'].map(id => ({ id, description: id === 'retry' ? 'Canonical retries apply once with accurate totals' : 'Invalid keys throw without consuming receipts',
      required: true, scope: ['src'], sourceIds: ['policy', 'checks'], checkIds: [id], evidenceKind: 'check' })),
    decisions: [], derivation: { reporter: 'fixture assessor', method: 'observed', observedAt: at } };
  const contractFile = path.join(root, '.git', 'contract.json');
  fs.writeFileSync(contractFile, JSON.stringify(contract));
  const start = () => command('task-start', '--task', 'receipt-fix', '--goal', contract.intent, '--file', 'src', '--contract', contractFile);
  const records = () => {
    // Capture the checked source before execution. A caller must assess check adequacy.
    const binding = command('session').binding;
    return contract.criteria.map(criterion => {
      const result = check(criterion.id);
      assert.equal(result.status, 0, result.stdout + result.stderr);
      const artifact = path.join(root, '.git', `${criterion.id}.log`);
      fs.writeFileSync(artifact, result.stdout + result.stderr);
      return { id: criterion.id, criterionIds: [criterion.id], scope: ['src'],
        contract: engineeringContractDigest(contract), binding, checkId: criterion.id, kind: 'check',
        artifact, observedAt: at, reporter: 'deterministic fixture assessor', basis: criterion.description,
        support: 'verified', execution: 'passed' };
    });
  };
  const submit = (records, complete = false) => {
    const expected = command('session');
    const file = path.join(root, '.git', 'ledger.json');
    fs.writeFileSync(file, JSON.stringify({ formatVersion: 1, records }));
    return command('task-step', '--stage', 'verify', '--target', 'acceptance:receipts', '--approach', 'separate-behavior-checks',
      '--evidence', 'retained fixture checks', '--outcome', 'passed', '--result', 'Assessed criterion coverage', '--next', 'Report gaps or finish',
      '--acceptance-evidence', file, '--expected-task-contract', expected.taskContract, '--expected-evidence-digest', expected.evidenceDigest,
      ...(complete ? ['--complete'] : []));
  };
  return { root, git, check, command, input, context, start, fix, records, submit };
}

test('developer slice reproduces failure, reuses helper and binds separate executable checks to completion', t => {
  const f = setup(t);
  assert.equal(f.check('retry').status, 1);
  assert.equal(f.check('invalid').status, 0);
  assert.equal(f.check('syntax').status, 0);
  assert.equal(f.context.constraints[0].status, 'accepted');
  assert.ok(f.context.sources.find(source => source.id === 'reuse').identity);
  assert.notEqual(f.context.coverage, 'complete');
  assert.equal(f.start().acceptance.complete, false);
  f.fix();
  const records = f.records();
  assert.equal(f.submit(records.slice(0, 1)).acceptance.complete, false);
  assert.throws(() => f.submit(records.slice(0, 1), true), /every required criterion/);
  assert.equal(f.submit(records, true).complete, true);
  assert.equal(f.command('session').task.goalComplete, true);
  assert.deepEqual(f.git('diff', '--name-only').split('\n'), ['src/receipt.mjs']);
  assert.equal(fs.existsSync(path.join(f.root, '.craft/project.json')), false);
});

test('developer slice keeps unavailable and irrelevant checks incomplete', t => {
  const f = setup(t); f.start(); f.fix();
  const records = f.records();
  for (const change of [record => ({ ...record, execution: 'not-available', support: 'unknown', artifact: 'unavailable: runner missing' }),
    record => ({ ...record, checkId: 'syntax', basis: 'Syntax passed; no invalid-input behavior checked' })]) {
    assert.equal(f.check('syntax').status, 0);
    const partial = [records[0], change(records[1])];
    assert.equal(f.submit(partial).acceptance.complete, false);
    assert.throws(() => f.submit(partial, true), /every required criterion/);
  }
});

test('developer slice rejects old results after implementation or instruction changes', t => {
  const f = setup(t); f.start(); f.fix();
  const records = f.records();
  f.submit(records, true);
  fs.appendFileSync(path.join(f.root, 'src/receipt.mjs'), '\n// Later edit requires rechecking.\n');
  assert.equal(f.command('session').acceptance.records[0].freshness, 'stale');
  assert.throws(() => f.submit(records, true), /every required criterion/);
  const refreshed = f.records();
  assert.equal(f.submit(refreshed, true).complete, true);
  fs.appendFileSync(path.join(f.root, 'AGENTS.md'), '\nChanged policy.\n');
  const context = groundedContext(f.root, f.input).context;
  assert.equal(context.constraints[0].status, 'unknown');
  assert.equal(f.command('session').task.goalComplete, false);
  assert.throws(() => f.submit(refreshed, true), /every required criterion/);
});

test('tiny developer fix works directly with no configuration, hooks or structured task', t => {
  const f = setup(t, false); f.fix();
  assert.equal(f.check('retry').status, 0);
  assert.equal(f.check('invalid').status, 0);
  assert.equal(fs.existsSync(path.join(f.root, '.craft')), false);
});
