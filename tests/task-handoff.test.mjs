import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { nativeCommand, run } from './craft.mjs';
import { hookPackage, runHook } from './hook-command.mjs';
import { stateLocation } from './state.mjs';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function fixture(t) {
  const root = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-handoff-')));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const git = (...args) => assert.equal(spawnSync('git', ['-C', root, '-c', 'core.hooksPath=/dev/null', '-c', 'commit.gpgsign=false',
    '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test', ...args]).status, 0);
  git('init', '-q');
  fs.writeFileSync(path.join(root, 'value.mjs'), 'export const value = 1;\n');
  git('add', '.'); git('commit', '-qm', 'fixture');
  return { root, git };
}

// The context Claude receives when `session` starts in `root`.
function started(root, session) {
  const result = runHook(hookPackage(path.join(repository, 'dist', 'claude'), 'claude', 'craft-delivery'), 'claude', 'restore',
    { cwd: root, input: JSON.stringify({ session_id: session, cwd: root, source: 'startup' }) });
  assert.equal(result.status, 0, result.stderr);
  return JSON.parse(result.stdout).hookSpecificOutput.additionalContext;
}

const handoffs = context => {
  const line = context.split('\n').find(text => text.startsWith('Unfinished tasks of earlier sessions'));
  return line ? JSON.parse(line.slice(line.indexOf(': [') + 2)) : [];
};

// Every file of a session directory with its content.
const contents = directory => Object.fromEntries(fs.readdirSync(directory).sort()
  .map(name => [name, fs.readFileSync(path.join(directory, name), 'utf8')]));

const step = (root, session, next = 'Run the value tests', extra = []) => run(['task-step', '--root', root, '--session', session,
  '--stage', 'implement', '--target', 'acceptance:value', '--approach', 'bounded-fix', '--evidence', 'checks/value-1',
  '--outcome', 'passed', '--result', 'Value corrected', '--next', next, ...extra]);

test('a new session learns its id, is offered the unfinished task and adopts a copy of it', t => {
  const { root } = fixture(t);
  run(['task-start', '--root', root, '--session', 'first', '--task', 'fix-value', '--goal', 'Correct the value', '--file', 'value.mjs']);
  step(root, 'first');
  const first = stateLocation(root, 'first');
  const before = contents(first.directory);

  const context = started(root, 'second');
  assert.match(context, /^Craft session \(pass as --session; no authorization implied\): \{"id":"second","task":null\}/);
  const [offered, ...others] = handoffs(context);
  assert.deepEqual(others, []);
  assert.deepEqual({ ...offered, updated: undefined }, { from: first.sessionId, task: 'fix-value', goal: 'Correct the value',
    stage: 'implement', nextStep: 'Run the value tests', updated: undefined, repositories: false });

  const adopted = run(['task-adopt', '--root', root, '--session', 'second', '--from', first.sessionId]);
  assert.equal(adopted.adopted, true);
  assert.equal(adopted.task, 'fix-value');
  assert.equal(adopted.context.nextStep, 'Run the value tests');
  assert.equal(adopted.context.stale, false);
  assert.equal(adopted.checkpoint.changed, false);
  assert.equal(adopted.checkpoint.saved.continuation.nextStep, 'Run the value tests');
  assert.deepEqual(adopted.notCarried, []);
  assert.deepEqual(contents(first.directory), before, 'the earlier session is unchanged');

  // Bound now: the session continues the task, and knows it at its next start.
  step(root, 'second', 'Review the change');
  assert.match(started(root, 'second'), /"id":"second","task":"fix-value"/);
  assert.deepEqual(handoffs(started(root, 'second')), []);
  assert.equal(run(['task-adopt', '--root', root, '--session', 'second', '--from', first.sessionId]).adopted, false);

  // A third session is offered the most recent record only.
  assert.deepEqual(handoffs(started(root, 'third')).map(item => [item.from, item.nextStep]),
    [[stateLocation(root, 'second').sessionId, 'Review the change']]);
  // Once completed there, neither copy is offered again.
  step(root, 'second', 'Report the change', ['--complete']);
  assert.deepEqual(handoffs(started(root, 'fourth')), []);
});

test('adoption refuses what it cannot take over without overwriting', t => {
  const { root, git } = fixture(t);
  run(['task-start', '--root', root, '--session', 'first', '--task', 'fix-value', '--goal', 'Correct the value']);
  const from = stateLocation(root, 'first').sessionId;
  const refused = (session, source) => {
    const outcome = nativeCommand(['task-adopt', '--root', root, '--session', session, '--from', source]);
    assert.notEqual(outcome.exitCode, 0);
    return outcome.stderr;
  };
  assert.match(refused('first', from), /cannot adopt its own task/);
  assert.match(refused('second', '0123456789abcdef01234567'), /no earlier session/);
  assert.match(refused('second', '../first'), /invalid session directory id/);
  run(['task-start', '--root', root, '--session', 'busy', '--task', 'other', '--goal', 'Other outcome']);
  assert.match(refused('busy', from), /task other is bound to this session/);
  // Bound to another checkout: not offered and not adoptable.
  git('checkout', '-qb', 'elsewhere');
  assert.deepEqual(handoffs(started(root, 'second')), []);
  assert.match(refused('second', from), /no unfinished task for this checkout/);
});
