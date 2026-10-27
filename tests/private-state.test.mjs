import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { deliverHook } from './native-hook.mjs';
import { saveCheckpoint, readCheckpoint } from './checkpoint-cli.mjs';
import { run } from './craft.mjs';
import { readProject } from './project-settings.mjs';
import { stateFile, stateLocation } from './state.mjs';

function fixture(t) {
  // State identity is the physical path; Windows temp paths may contain 8.3 short names.
  const root = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-private-state-')));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const git = (...args) => {
    const result = spawnSync('git', ['-C', root, '-c', 'core.hooksPath=/dev/null', '-c', 'commit.gpgsign=false', '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test', ...args], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr); return result.stdout.trim();
  };
  git('init', '-q');
  // Keep Git's background writers out of before/after storage observations.
  git('config', 'gc.auto', '0');
  git('config', 'maintenance.auto', 'false');
  fs.mkdirSync(path.join(root, 'src')); fs.writeFileSync(path.join(root, 'src/value.mjs'), 'export const SymbolValue = 1;\n');
  git('add', '.'); git('commit', '-qm', 'fixture');
  return { root, git };
}
// One native delivery hook event; the structured decision, before host serialization.
const deliver = (cwd, lifecycle, { session = 's1', ...options } = {}) => deliverHook(cwd, lifecycle, { session, ...options });
const continuation = Object.fromEntries(['goal', 'currentState', 'completed', 'approach', 'evidence', 'filesChanged', 'validation', 'remainingWork', 'blockers', 'nextStep'].map(key => [key, key]));

test('default lifecycle and discovery use excluded local state without editing consumer ignores', t => {
  const { root, git } = fixture(t);
  const nested = path.join(root, 'src');
  assert.equal(readProject(root).routing.editHints, true);
  const route = deliver(nested, 'before-tool', { tool: 'edit', file: path.join(nested, 'value.mjs') });
  assert.equal(route.context.data.intent, 'implement');
  deliver(nested, 'turn-end');
  assert.equal(readCheckpoint(root, 's1').head, git('rev-parse', 'HEAD'));
  const locate = (task = 'task-1') => run(['locate', '--root', root, '--session', 's1', '--task', task, '--term', 'SymbolValue']).discovery;
  assert.equal(locate().fresh, 1);
  assert.equal(locate().reused, 1);
  assert.equal(locate('task-2').reused, 0);
  fs.appendFileSync(path.join(nested, 'value.mjs'), '// source change\n');
  assert.equal(locate().reused, 0);
  assert.match(stateFile(root, 'probe.json'), /\.craft[/\\]state[/\\]probe\.json$/);
  assert.equal(fs.existsSync(path.join(root, '.git', 'craft-state')), false);
  assert.match(git('status', '--porcelain'), /M src\/value.mjs/);
  assert.match(git('status', '--porcelain'), /\?\? \.craft\//);
});

test('edits outside the project root skip edit-hint routing instead of failing delivery', t => {
  const { root } = fixture(t);
  const nested = path.join(root, 'src');
  const outside = path.join(path.dirname(root), 'elsewhere', 'notes.md');
  // Without a session, hint reuse cannot suppress the repeated routes compared below.
  const event = file => deliver(nested, 'before-tool', { tool: 'edit', file, host: 'claude', session: null });
  assert.equal(event(outside).context, undefined);
  assert.equal(event(path.join(root, '..', path.basename(root), 'src', 'value.mjs')).context.data.intent, 'implement');
  // A relative tool path names a file below the hook's working directory.
  assert.deepEqual(event('value.mjs').context, event(path.join(nested, 'value.mjs')).context);
  assert.equal(event('../../elsewhere/notes.md').context, undefined);
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
});

test('state identity follows the physical worktree, not the caller spelling', t => {
  const { root } = fixture(t);
  const variant = path.join(path.dirname(root), path.basename(root).toUpperCase());
  if (!fs.existsSync(variant)) return t.skip('case-sensitive filesystem');
  const saved = saveCheckpoint(variant, 'session');
  assert.equal(saveCheckpoint(root, 'session').file, saved.file);
  assert.equal(stateOf(saved.file), rootState(root));
});

test('linked worktrees isolate session state and metadata symlinks are rejected', t => {
  const { root, git } = fixture(t);
  const linked = path.join(root, 'linked');
  git('worktree', 'add', '--detach', linked, 'HEAD');
  const first = saveCheckpoint(root, 'same-session');
  const second = saveCheckpoint(linked, 'same-session');
  assert.notEqual(first.file, second.file);
  assert.ok(second.file.startsWith(path.join(linked, '.craft', 'state') + path.sep));
  assert.notEqual(stateOf(first.file), stateOf(second.file));
  const directory = path.dirname(first.file);
  fs.rmSync(directory, { recursive: true });
  fs.symlinkSync(path.join(root, 'src'), directory, process.platform === 'win32' ? 'junction' : 'dir');
  assert.throws(() => saveCheckpoint(root, 'same-session'), /symlink/);
});

test('personal reuse controls affect CLI discovery and preference changes invalidate packets', t => {
  const { root } = fixture(t);
  const preferencesFile = path.join(root, '.git', 'preferences-fixture.json');
  fs.writeFileSync(preferencesFile, '{}', { mode: 0o600 });
  const discover = () => run(['locate', '--root', root, '--session', 's1', '--task', 'task-1', '--preferences', preferencesFile, '--term', 'SymbolValue']);
  discover();
  assert.equal(discover().discovery.reused, 1);
  fs.writeFileSync(preferencesFile, JSON.stringify({ efficiency: { lookup: { reuse: false } } }));
  const disabled = discover();
  assert.equal(disabled.reuse.enabled, false);
  assert.equal(disabled.discovery.reused, 0);
  fs.writeFileSync(preferencesFile, JSON.stringify({ routing: { editHints: false } }));
  assert.equal(discover().discovery.reused, 0);
  assert.equal(discover().discovery.reused, 1);
});

const rootState = root => stateLocation(root).directory;
// `<state>/sessions/<session>/<record>`
function stateOf(file) { return path.dirname(path.dirname(path.dirname(file))); }
test('explicit root and session IDs give Git and non-Git roots the same storage model', t => {
  const { root, git } = fixture(t);
  const plain = path.join(root, 'coordination'); fs.mkdirSync(plain);
  const files = [];
  for (const selected of [root, plain]) {
    const a = saveCheckpoint(selected, 'a', { taskId: 'task-a' }).file;
    const b = path.join(path.dirname(path.dirname(a)), stateLocation(selected, 'b').sessionId, path.basename(a));
    assert.notEqual(path.dirname(a), path.dirname(b));
    assert.equal(stateOf(a), rootState(selected));
    assert.equal(path.basename(path.dirname(path.dirname(a))), 'sessions');
    assert.match(path.basename(path.dirname(a)), /^[a-f0-9]{24}$/);
    const { state } = run(['settings', '--root', selected]);
    assert.deepEqual([state.scope, state.formatVersion], ['workspace-local', 4]);
    assert.equal(a.includes(`${path.sep}.git${path.sep}`), false);
    assert.equal(readCheckpoint(selected, 'b'), null);
    files.push(a);
  }
  fs.writeFileSync(path.join(root, '.gitignore'), '**/.craft/state/\n');
  for (const file of files) git('check-ignore', '--quiet', path.relative(root, file));
});

// Legacy record migration, its refusals and Git metadata immutability:
// `cargo test -p craft-platform state::`.
test('a migrated checkpoint retains its session history and remains writable', t => {
  const { root } = fixture(t);
  const saved = saveCheckpoint(root, 'migrated-session', { taskId: 'task', continuation });
  const old = path.join(root, '.git', 'craft-state', stateLocation(root).worktreeId); fs.mkdirSync(old, { recursive: true });
  const legacyFile = path.join(old, `${stateLocation(root, 'migrated-session').sessionId}.json`);
  fs.copyFileSync(saved.file, legacyFile); fs.unlinkSync(saved.file);
  assert.deepEqual(readCheckpoint(root, 'migrated-session'), saved.state);
  const updated = saveCheckpoint(root, 'migrated-session', { taskId: 'next-task' });
  assert.equal(updated.state.taskId, 'next-task');
  assert.equal(JSON.parse(fs.readFileSync(legacyFile, 'utf8')).taskId, 'task');
  assert.equal(readCheckpoint(root, 'other-session'), null);
});
