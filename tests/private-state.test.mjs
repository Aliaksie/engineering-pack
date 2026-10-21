import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { stateFile, projectRoot } from '../platform/index.mjs';
import { prepareEvent } from '../application/context-delivery/prepare.mjs';
import { saveCheckpoint, readCheckpoint } from '../application/checkpoint.mjs';
import { discover } from '../application/discovery/discover.mjs';
import { readProject } from '../application/project/configuration.mjs';

function fixture(t) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-private-state-')));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const git = (...args) => {
    const result = spawnSync('git', ['-C', root, '-c', 'core.hooksPath=/dev/null', '-c', 'commit.gpgsign=false', '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test', ...args], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr); return result.stdout.trim();
  };
  git('init', '-q');
  fs.mkdirSync(path.join(root, 'src')); fs.writeFileSync(path.join(root, 'src/value.mjs'), 'export const SymbolValue = 1;\n');
  git('add', '.'); git('commit', '-qm', 'fixture');
  return { root, git };
}
const continuation = Object.fromEntries(['goal', 'currentState', 'completed', 'approach', 'evidence', 'filesChanged', 'validation', 'remainingWork', 'blockers', 'nextStep'].map(key => [key, key]));

test('default lifecycle and discovery stay private in a fresh repository without config or ignores', t => {
  const { root, git } = fixture(t);
  const nested = path.join(root, 'src');
  assert.equal(projectRoot(nested), root);
  assert.equal(readProject(root).routing.editHints, true);
  const route = prepareEvent({ cwd: nested, event: 'before-tool', tool: 'edit', file: path.join(nested, 'value.mjs'), host: 'codex', session: 's1' });
  assert.equal(route.routing.intent, 'implement');
  prepareEvent({ cwd: nested, event: 'turn-end', tool: 'other', host: 'codex', session: 's1' });
  assert.equal(readCheckpoint(root, 's1').head, git('rev-parse', 'HEAD'));
  const scope = { session: 's1', task: 'task-1' };
  assert.equal(discover(root, { terms: ['SymbolValue'] }, scope).discovery.fresh, 1);
  assert.equal(discover(root, { terms: ['SymbolValue'] }, scope).discovery.reused, 1);
  assert.equal(discover(root, { terms: ['SymbolValue'] }, { ...scope, task: 'task-2' }).discovery.reused, 0);
  fs.appendFileSync(path.join(nested, 'value.mjs'), '// source change\n');
  assert.equal(discover(root, { terms: ['SymbolValue'] }, scope).discovery.reused, 0);
  assert.match(stateFile(root, 'probe.json'), /\.git[/\\]craft-state[/\\]/);
  assert.equal(fs.existsSync(path.join(root, '.craft')), false);
  assert.equal(git('status', '--porcelain'), 'M src/value.mjs');
});

test('unchanged snapshots are deduplicated and task transitions discard old continuation', t => {
  const { root } = fixture(t);
  const first = saveCheckpoint(root, 's1', { taskId: 'task-1', continuation });
  const before = fs.statSync(first.file).mtimeMs;
  const repeated = saveCheckpoint(root, 's1');
  assert.equal(repeated.unchanged, true);
  assert.equal(fs.statSync(first.file).mtimeMs, before);
  assert.deepEqual(repeated.state.continuation, continuation);
  const next = saveCheckpoint(root, 's1', { taskId: 'task-2' });
  assert.equal(next.state.continuation, undefined);
  assert.equal(next.state.continuationSource, undefined);
  assert.notEqual(stateFile(root, 's1.json'), stateFile(path.join(root, 'src'), 's1.json'));
});

test('linked worktrees isolate session state and metadata symlinks are rejected', t => {
  const { root, git } = fixture(t);
  const linked = path.join(root, 'linked');
  git('worktree', 'add', '--detach', linked, 'HEAD');
  const first = saveCheckpoint(root, 'same-session');
  const second = saveCheckpoint(linked, 'same-session');
  assert.notEqual(first.file, second.file);
  assert.ok(second.file.includes(`${path.sep}worktrees${path.sep}`));
  const directory = path.dirname(stateFile(root, 'probe.json'));
  fs.rmSync(directory, { recursive: true });
  fs.symlinkSync(path.join(root, 'src'), directory, process.platform === 'win32' ? 'junction' : 'dir');
  assert.throws(() => stateFile(root, 'probe.json', true), /symlinks/);
});

test('personal reuse controls affect CLI discovery and preference changes invalidate packets', t => {
  const { root } = fixture(t);
  const preferencesFile = path.join(root, '.git', 'preferences-fixture.json');
  fs.writeFileSync(preferencesFile, '{}', { mode: 0o600 });
  const scope = { session: 's1', task: 'task-1', preferencesFile };
  const request = { terms: ['SymbolValue'] };
  discover(root, request, scope);
  assert.equal(discover(root, request, scope).discovery.reused, 1);
  fs.writeFileSync(preferencesFile, JSON.stringify({ efficiency: { lookup: { reuse: false } } }));
  const disabled = discover(root, request, scope);
  assert.equal(disabled.reuse.enabled, false);
  assert.equal(disabled.discovery.reused, 0);
  fs.writeFileSync(preferencesFile, JSON.stringify({ routing: { editHints: false } }));
  assert.equal(discover(root, request, scope).discovery.reused, 0);
  assert.equal(discover(root, request, scope).discovery.reused, 1);
  assert.throws(() => stateFile(root, '..', true), /filename/);
});
