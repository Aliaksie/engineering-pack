import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { createHash } from 'node:crypto';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { stateFile, stateLocation, readState, writeState, projectRoot } from '../platform/index.mjs';
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

test('default lifecycle and discovery use excluded local state without editing consumer ignores', t => {
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
  assert.match(stateFile(root, 'probe.json'), /\.craft[/\\]state[/\\][a-f0-9]{24}[/\\]shared[/\\]/);
  assert.equal(fs.existsSync(path.join(root, '.git', 'craft-state')), false);
  assert.match(git('status', '--porcelain'), /M src\/value.mjs/);
  assert.match(git('status', '--porcelain'), /\?\? \.craft\//);
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
  assert.ok(second.file.startsWith(path.join(linked, '.craft', 'state') + path.sep));
  assert.notEqual(path.basename(path.dirname(path.dirname(first.file))), path.basename(path.dirname(path.dirname(second.file))));
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

// These storage tests exercise real files/Git worktrees rather than mocked paths.
const rootId = root => createHash('sha256').update(fs.realpathSync(root)).digest('hex').slice(0, 24);
function metadata(root) {
  const result = {};
  const walk = directory => {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      const file = path.join(directory, entry.name);
      if (entry.isDirectory()) walk(file);
      else result[path.relative(root, file)] = [fs.statSync(file).mtimeMs, fs.readFileSync(file).toString('base64')];
    }
  };
  walk(path.join(root, '.git')); return result;
}

test('explicit root and session IDs give Git and non-Git roots the same storage model', t => {
  const { root, git } = fixture(t);
  const plain = path.join(root, 'coordination'); fs.mkdirSync(plain);
  for (const selected of [root, plain]) {
    const a = stateFile(selected, 'record.json', true, 'a');
    const b = stateFile(selected, 'record.json', true, 'b');
    assert.notEqual(a, b);
    assert.equal(path.basename(path.dirname(path.dirname(a))), rootId(selected));
    assert.match(path.basename(path.dirname(a)), /^[a-f0-9]{24}$/);
    assert.equal(stateLocation(selected).scope, 'workspace-local');
    assert.equal(stateLocation(selected).formatVersion, 2);
    assert.equal(a.includes(`${path.sep}.git${path.sep}`), false);
    writeState(a, { session: 'a' }, 1024);
    assert.equal(readState(b, 1024), null);
  }
  fs.writeFileSync(path.join(root, '.gitignore'), '**/.craft/state/\n');
  for (const selected of [root, plain]) {
    git('check-ignore', '--quiet', path.relative(root, stateFile(selected, 'record.json')));
  }
});

for (const legacy of ['git', 'craft', 'capabilities']) test(`${legacy} legacy records migrate without Git metadata or configuration writes`, t => {
  const { root } = fixture(t);
  fs.mkdirSync(path.join(root, '.craft'));
  const config = path.join(root, '.craft', 'project.json');
  const telemetryConfig = path.join(root, '.craft', 'telemetry.json');
  fs.writeFileSync(config, '{"consumer":"preserve"}'); fs.writeFileSync(telemetryConfig, '{"local":false}');
  const old = legacy === 'git' ? path.join(root, '.git', 'craft-state', rootId(root)) : path.join(root, legacy === 'craft' ? '.craft' : '.capabilities', 'state');
  fs.mkdirSync(old, { recursive: true });
  fs.writeFileSync(path.join(old, 'record.json'), '{"history":[1,2]}\n');
  fs.writeFileSync(path.join(old, 'telemetry.jsonl'), '{"schemaVersion":1}\n');
  fs.writeFileSync(path.join(old, 'telemetry.jsonl.1'), '{"schemaVersion":1,"older":true}\n');
  fs.mkdirSync(path.join(old, 'sessions', 'owned', 'evidence'), { recursive: true });
  fs.writeFileSync(path.join(old, 'sessions', 'owned', 'evidence', 'artifact.txt'), 'retained evidence');
  const before = metadata(root);
  const file = stateFile(root, 'record.json', false, 'existing-session');
  assert.deepEqual(readState(file, 1024), { history: [1, 2] });
  writeState(file, { history: [1, 2, 3] }, 1024);
  assert.equal(stateFile(root, 'record.json', false, 'existing-session'), file);
  const log = stateFile(root, 'telemetry.jsonl');
  assert.equal(fs.readFileSync(`${log}.1`, 'utf8'), '{"schemaVersion":1,"older":true}\n');
  const workspace = stateFile(root, 'workspace-guard.json');
  assert.equal(fs.readFileSync(path.join(path.dirname(workspace), 'sessions/owned/evidence/artifact.txt'), 'utf8'), 'retained evidence');
  assert.deepEqual(metadata(root), before);
  assert.equal(fs.readFileSync(path.join(old, 'record.json'), 'utf8'), '{"history":[1,2]}\n');
  assert.equal(fs.readFileSync(config, 'utf8'), '{"consumer":"preserve"}');
  assert.equal(fs.readFileSync(telemetryConfig, 'utf8'), '{"local":false}');
  fs.writeFileSync(path.join(old, 'record.json'), '{"history":[4]}');
  assert.throws(() => stateFile(root, 'record.json', false, 'existing-session'), /changed after migration/);
  assert.deepEqual(readState(file, 1024), { history: [1, 2, 3] });
});

test('migration refuses old locks, malformed records, symlinks and destination conflicts', t => {
  const { root } = fixture(t);
  const legacy = path.join(root, '.craft', 'state'); fs.mkdirSync(legacy, { recursive: true });
  const old = path.join(legacy, 'record.json');
  const target = stateFile(root, 'record.json', true, 'session');
  fs.writeFileSync(old, '{"value":1}');
  fs.writeFileSync(`${old}.lock`, '');
  assert.throws(() => stateFile(root, 'record.json', true, 'session'), /legacy state writer is busy/);
  fs.unlinkSync(`${old}.lock`);
  fs.writeFileSync(old, 'broken');
  assert.throws(() => stateFile(root, 'record.json', true, 'session'), SyntaxError);
  fs.unlinkSync(old); fs.symlinkSync(target, old);
  assert.throws(() => stateFile(root, 'record.json', true, 'session'), /unsafe/);
  fs.unlinkSync(old); fs.writeFileSync(old, '{"value":1}'); fs.writeFileSync(target, '{"value":2}');
  assert.throws(() => stateFile(root, 'record.json', true, 'session'), /destination conflict/);
  assert.equal(fs.readFileSync(target, 'utf8'), '{"value":2}');
  // A matching file without a published receipt represents an interrupted copy.
  fs.writeFileSync(target, '{"value":1}');
  assert.equal(stateFile(root, 'record.json', true, 'session'), target);
  assert.deepEqual(readState(target, 1024), { value: 1 });
});

test('fresh runtime writes never create or change Git metadata', t => {
  const { root } = fixture(t);
  const before = metadata(root);
  const file = stateFile(root, 'record.json', true, 'session');
  writeState(file, { value: 1 }, 1024);
  assert.deepEqual(metadata(root), before);
});

test('interrupted migration retries identical copies and excludes overlapping migration writers', t => {
  const { root } = fixture(t);
  const legacy = path.join(root, '.craft', 'state'); fs.mkdirSync(legacy, { recursive: true });
  fs.writeFileSync(path.join(legacy, 'record.json'), '{"history":[1]}');
  const rename = fs.renameSync, link = fs.linkSync;
  let interrupted = false, overlapped = false;
  t.mock.method(fs, 'linkSync', (source, target) => {
    overlapped = true;
    assert.throws(() => stateFile(root, 'record.json', true, 'session'), /migration is busy/);
    return link(source, target);
  });
  t.mock.method(fs, 'renameSync', (source, target) => {
    if (path.basename(target).startsWith('migration-') && !interrupted) {
      interrupted = true; throw new Error('injected interruption before receipt');
    }
    return rename(source, target);
  });
  assert.throws(() => stateFile(root, 'record.json', true, 'session'), /injected interruption/);
  const file = stateFile(root, 'record.json', true, 'session');
  assert.equal(overlapped, true);
  assert.deepEqual(readState(file, 1024), { history: [1] });
  assert.ok(fs.readdirSync(path.dirname(file)).every(name => !name.endsWith('.tmp') && !name.endsWith('.lock')));
});

test('a migrated checkpoint retains its session history and remains writable', t => {
  const { root } = fixture(t);
  const saved = saveCheckpoint(root, 'migrated-session', { taskId: 'task', continuation });
  const old = path.join(root, '.git', 'craft-state', rootId(root)); fs.mkdirSync(old, { recursive: true });
  const legacyFile = path.join(old, path.basename(saved.file));
  fs.copyFileSync(saved.file, legacyFile); fs.unlinkSync(saved.file);
  assert.deepEqual(readCheckpoint(root, 'migrated-session'), saved.state);
  const updated = saveCheckpoint(root, 'migrated-session', { taskId: 'next-task' });
  assert.equal(updated.state.taskId, 'next-task');
  assert.equal(JSON.parse(fs.readFileSync(legacyFile, 'utf8')).taskId, 'task');
  assert.equal(readCheckpoint(root, 'other-session'), null);
});

test('linked-worktree migration reads its own legacy Git directory and leaves it unchanged', t => {
  const { root, git } = fixture(t);
  const linked = path.join(root, 'linked'); git('worktree', 'add', '--detach', linked, 'HEAD');
  const gitDirectory = path.resolve(linked, fs.readFileSync(path.join(linked, '.git'), 'utf8').trim().slice(8));
  const legacy = path.join(gitDirectory, 'craft-state', rootId(linked)); fs.mkdirSync(legacy, { recursive: true });
  fs.writeFileSync(path.join(legacy, 'record.json'), '{"worktree":"linked"}');
  const before = metadata(root);
  const file = stateFile(linked, 'record.json', false, 'linked-session');
  assert.deepEqual(readState(file, 1024), { worktree: 'linked' });
  assert.equal(readState(stateFile(root, 'record.json', false, 'linked-session'), 1024), null);
  assert.deepEqual(metadata(root), before);
});
