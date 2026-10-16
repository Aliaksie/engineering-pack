import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { HOSTS } from '../adapters/shared/hosts.mjs';
import { saveCheckpoint, readCheckpoint } from '../runtime/checkpoint.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const native = ['claude', 'codex', 'cursor', 'copilot', 'gemini'];
function fixture(t, config = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'host hooks '));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  fs.mkdirSync(path.join(dir, '.capabilities'));
  fs.writeFileSync(path.join(dir, '.capabilities/project.json'), JSON.stringify({ formatVersion: 1, ...config }));
  return dir;
}
function payload(host, cwd, command, tool = 'shell') {
  if (host === 'copilot') return { sessionId: 'fixture-session', cwd, toolName: tool === 'shell' ? 'bash' : 'edit', toolArgs: { command, path: 'orders.py' } };
  return { session_id: 'fixture-session', cwd, tool_name: tool === 'shell' ? host === 'cursor' ? 'Shell' : host === 'gemini' ? 'run_shell_command' : 'Bash' : 'Write', tool_input: { command, file_path: 'orders.py' } };
}
function invoke(host, pkg, hook, input, cwd) {
  return spawnSync(process.execPath, [path.join(root, 'dist', host, host === 'claude' ? 'plugins' : 'packages', pkg, `platform-hooks/${hook}.mjs`)], { cwd, input: typeof input === 'string' ? input : JSON.stringify(input), encoding: 'utf8' });
}
function decision(host, result) {
  if (host === 'claude') return result.status === 2 ? 'deny' : 'allow';
  const output = result.stdout.trim() ? JSON.parse(result.stdout) : {};
  return output.hookSpecificOutput?.permissionDecision ?? output.permissionDecision ?? output.permission ?? output.decision ?? 'allow';
}

test('all native wrappers map read-only review denials without imposing a global restriction', (t) => {
  const guarded = fixture(t, { review: { readOnly: true } });
  const unrestricted = fixture(t);
  for (const host of native) {
    for (const command of ['gh pr comment 42 --body hi', 'glab mr approve 42', 'git push origin main']) {
      const result = invoke(host, 'craft-review', 'read-only', payload(host, guarded, command), guarded);
      assert.equal(decision(host, result), 'deny', `${host}: ${result.stderr}`);
      assert.equal(result.status, host === 'claude' ? 2 : 0);
    }
    assert.equal(decision(host, invoke(host, 'craft-review', 'read-only', payload(host, guarded, 'git status --short'), guarded)), 'allow');
    assert.equal(decision(host, invoke(host, 'craft-review', 'read-only', payload(host, unrestricted, 'gh pr comment 42 --body hi'), unrestricted)), 'allow');
  }
});

test('malformed payloads and unrelated tools produce valid nonblocking wire responses', (t) => {
  const dir = fixture(t, { review: { readOnly: true } });
  for (const host of native) for (const input of ['bad json', 'null', '[]', JSON.stringify({ ...payload(host, dir, 'gh pr comment 1'), tool_name: 'Read', toolName: 'view' })]) {
    const result = invoke(host, 'craft-review', 'read-only', input, dir);
    assert.equal(result.status, 0, `${host}: ${result.stderr}`);
    assert.equal(decision(host, result), 'allow');
  }
});

test('prompt routing hints appear only where the native context schema supports them', (t) => {
  const dir = fixture(t);
  for (const host of native) {
    const result = invoke(host, 'craft-delivery', 'prompt-routing', { cwd: dir, session_id: 'fixture-session', prompt: 'review this change' }, dir);
    assert.equal(result.status, 0, result.stderr);
    const output = result.stdout.trim() ? JSON.parse(result.stdout) : {};
    const context = output.hookSpecificOutput?.additionalContext ?? output.additionalContext ?? output.additional_context ?? '';
    assert.equal(context.includes('Routing hints'), HOSTS[host].events['before-prompt'].context, host);
    if (context) assert.ok(context.includes(host === 'claude' ? 'craft-review:review-change' : 'craft-review-review-change'));
  }
});

test('edit routing is opt-in and prompt hints omit the full catalog evidence', (t) => {
  const quiet = fixture(t);
  const enabled = fixture(t, { routing: { editHints: true } });
  for (const host of native) {
    const ordinary = invoke(host, 'craft-delivery', 'routing', payload(host, quiet, '', 'edit'), quiet);
    assert.equal(ordinary.status, 0, ordinary.stderr);
    assert.ok(!ordinary.stdout.includes('Routing hints'), host);
    const opted = invoke(host, 'craft-delivery', 'routing', payload(host, enabled, '', 'edit'), enabled);
    assert.equal(opted.status, 0, opted.stderr);
    assert.equal(opted.stdout.includes('Routing hints'), HOSTS[host].events['before-tool'].context, host);
    assert.ok(!opted.stdout.includes('documentationQuestions'), 'full route remains available through the CLI');
    assert.ok(!opted.stdout.includes('selections'), 'catalog evidence must not be injected on every edit');
  }
});

test('session service preparation reads metadata without probing executable availability', (t) => {
  const dir = fixture(t, { services: { git: { provider: 'git', transport: 'cli', command: 'never-execute-this-command' } } });
  for (const host of native) {
    const result = invoke(host, 'craft-delivery', 'restore', { cwd: dir, session_id: 'fixture-session' }, dir);
    assert.equal(result.status, 0, result.stderr);
    assert.ok(result.stdout.includes('never-execute-this-command'), host);
    assert.ok(result.stdout.includes('unverified'), host);
    assert.ok(!fs.existsSync(path.join(dir, '.capabilities/state')), 'metadata preparation creates no cache/authorization state');
  }
});

test('compaction observers save checkpoints without claiming to control compression', (t) => {
  const dir = fixture(t, { checkpoint: { enabled: true } });
  saveCheckpoint(dir, 'fixture-session', { task: 'Fixture task', authorization: 'local review', nextStep: 'inspect snapshot' });
  for (const host of native) {
    const result = invoke(host, 'craft-delivery', 'checkpoint', { cwd: dir, session_id: 'fixture-session', trigger: 'auto' }, dir);
    assert.equal(result.status, 0, `${host}: ${result.stderr}`);
    assert.equal(readCheckpoint(dir, 'fixture-session').task, 'Fixture task');
    const output = result.stdout.trim() ? JSON.parse(result.stdout) : {};
    assert.ok(output.continue !== false && output.decision !== 'block');
  }
});

test('checkpoint state is not written without explicit consumer opt-in', (t) => {
  const dir = fixture(t);
  for (const host of native) {
    const result = invoke(host, 'craft-delivery', 'checkpoint', { cwd: dir, session_id: 'fixture-session' }, dir);
    assert.equal(result.status, 0, result.stderr);
  }
  assert.ok(!fs.existsSync(path.join(dir, '.capabilities/state')));
});

test('unsupported lifecycle mappings are reported and no fake wrapper is installed', () => {
  for (const host of ['cursor', 'copilot', 'gemini', 'agents']) {
    const output = path.join(root, 'dist', host);
    const report = JSON.parse(fs.readFileSync(path.join(output, 'adapter-report.json'), 'utf8'));
    const delivery = report.packages.find((entry) => entry.package === 'craft-delivery');
    assert.equal(delivery.hookMappings.find((entry) => entry.id === 'compact-restore').supported, false);
    assert.ok(!fs.existsSync(path.join(output, 'packages/craft-delivery/platform-hooks/compact-restore.mjs')));
  }
});


test('Claude and Codex restore context through the documented compact session-start event', (t) => {
  const dir = fixture(t, { checkpoint: { enabled: true } });
  saveCheckpoint(dir, 'fixture-session', { task: 'Resume only the scoped review', authorization: 'read-only', nextStep: 'inspect retry code' });
  for (const host of ['claude', 'codex']) {
    const result = invoke(host, 'craft-delivery', 'compact-restore', { cwd: dir, source: 'compact', session_id: 'fixture-session' }, dir);
    assert.equal(result.status, 0, result.stderr);
    const output = JSON.parse(result.stdout).hookSpecificOutput;
    assert.equal(output.hookEventName, 'SessionStart');
    assert.match(output.additionalContext, /Resume only the scoped review/);
    assert.equal(HOSTS[host].events['after-compact'].matcher, 'compact');
  }
});

test('Copilot declares the same Node hook for Bash and PowerShell', () => {
  const config = JSON.parse(fs.readFileSync(path.join(root, 'dist/copilot/.github/hooks/craft-capabilities.json'), 'utf8'));
  for (const hooks of Object.values(config.hooks)) for (const hook of hooks) {
    assert.equal(hook.powershell, hook.bash);
    assert.match(hook.powershell, /^node "\.\/packages\//);
    assert.ok(!/\.(?:sh|ps1)\b/.test(hook.powershell));
  }
});

test('PowerShell shell tools cannot bypass read-only review normalization', (t) => {
  const dir = fixture(t, { review: { readOnly: true } });
  for (const name of ['powershell', 'pwsh']) {
    const result = invoke('copilot', 'craft-review', 'read-only', payload('copilot', dir, 'git push origin main', name), dir);
    // payload() normally maps non-shell tool names to edit; exercise native toolName explicitly.
    const native = invoke('copilot', 'craft-review', 'read-only', { cwd: dir, toolName: name, toolArgs: { command: 'git push origin main' } }, dir);
    assert.equal(native.status, 0, native.stderr);
    assert.equal(decision('copilot', native), 'deny');
    assert.equal(decision('copilot', result), 'allow', 'unrelated edit payload stays out of shell policy');
  }
});
