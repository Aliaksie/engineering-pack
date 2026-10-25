import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { nativeTargets } from '../tooling/native-release.mjs';
import { hookPackage, runHook } from './hook-command.mjs';

const repo = fileURLToPath(new URL('../', import.meta.url));
const windows = process.platform === 'win32';
const current = nativeTargets.find(entry => entry.platform === process.platform && entry.arch === process.arch);
const { CRAFT_NATIVE_BINARY: binary, ...plain } = process.env;
const read = file => JSON.parse(fs.readFileSync(file, 'utf8'));

// An export copied under a path with spaces, as hosts install it.
function exported(t, host) {
  const scratch = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'craft hook launchers ')));
  t.after(() => fs.rmSync(scratch, { recursive: true, force: true }));
  const root = path.join(scratch, `${host} export`);
  // Exports are tested without bundled runtimes unless a test assembles one.
  fs.cpSync(path.join(repo, 'dist', host), root, { recursive: true, filter: source => path.basename(source) !== 'native-bin' });
  const project = path.join(scratch, 'project');
  fs.mkdirSync(path.join(project, '.craft'), { recursive: true });
  fs.writeFileSync(path.join(project, '.craft/project.json'), JSON.stringify({ formatVersion: 1, review: { readOnly: true } }));
  return { root, project };
}
// Place the runtime under test where bundle assembly installs it, without execute permission.
function assemble(packageRoot) {
  const target = path.join(packageRoot, 'support/native-bin', current.target, 'native-bin', `craft${windows ? '.exe' : ''}`);
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.copyFileSync(binary, target);
  if (!windows) fs.chmodSync(target, 0o644);
}
const publish = cwd => JSON.stringify({ session_id: 'S-1', cwd, tool_name: 'Bash', tool_input: { command: 'gh pr comment 1 --body review' } });

test('a launcher runs the bundled runtime for this platform without Node or an override', { skip: !current && 'no artifact for this platform' }, t => {
  const { root, project } = exported(t, 'claude');
  const packageRoot = hookPackage(root, 'claude', 'craft-review');
  assemble(packageRoot);
  // Only system tools: the hook path itself must not need Node.js.
  const env = { ...plain, PATH: windows ? plain.PATH : '/usr/bin:/bin' };
  if (!windows) assert.notEqual(spawnSync('sh', ['-c', 'command -v node'], { env }).status, 0, 'test PATH must not provide node');
  const denied = runHook(packageRoot, 'claude', 'read-only', { cwd: project, env, input: publish(project) });
  assert.equal(denied.status, 2, denied.stderr);
  assert.match(denied.stderr, /read-only review/);
  const allowed = runHook(packageRoot, 'claude', 'read-only', { cwd: project, env, input: JSON.stringify({ cwd: project, tool_name: 'Bash', tool_input: { command: 'git status' } }) });
  assert.equal(allowed.status, 0, allowed.stderr);
  assert.equal(allowed.stdout, '');
  // A damaged executable that cannot start or crashes fails shell policy closed.
  fs.writeFileSync(path.join(packageRoot, 'support/native-bin', current.target, 'native-bin', `craft${windows ? '.exe' : ''}`), 'damaged');
  const damaged = runHook(packageRoot, 'claude', 'read-only', { cwd: project, env, input: publish(project) });
  assert.equal(damaged.status, 2, damaged.stderr);
  assert.match(damaged.stderr, /Policy could not be evaluated: bundled native runtime unavailable/);
});

test('without a bundled runtime, launchers answer as a failed runtime would', t => {
  for (const host of ['claude', 'codex', 'cursor', 'copilot', 'gemini']) {
    const { root, project } = exported(t, host);
    const pkg = name => hookPackage(root, host, name);
    const run = (name, hook, input) => runHook(pkg(name), host, hook, { cwd: project, env: plain, input });
    const denied = run('craft-review', 'read-only', publish(project));
    if (host === 'claude') {
      assert.equal(denied.status, 2);
      assert.match(denied.stderr, /^Policy could not be evaluated: bundled native runtime unavailable\r?\n$/);
    } else {
      assert.equal(denied.status, 0, denied.stderr);
      const output = JSON.parse(denied.stdout);
      assert.equal(output.hookSpecificOutput?.permissionDecision ?? output.permissionDecision ?? output.permission ?? output.decision, 'deny', denied.stdout);
    }
    const observed = run('craft-engineering', 'observe-before-tool', publish(project));
    assert.deepEqual([observed.status, observed.stdout, observed.stderr], [0, '', '']);
    const started = run('craft-delivery', 'restore', JSON.stringify({ session_id: 'S-1', cwd: project }));
    assert.equal(started.status, 0, started.stderr);
    assert.match(started.stdout, /Routing\/checkpoint unavailable: bundled native runtime unavailable/);
    assert.ok(!fs.existsSync(path.join(project, '.craft/state')), 'no runtime ran');
  }
});

// Each registered command string through the shells a host may use. Claude runs hooks with
// Bash (Git Bash on Windows) and Copilot names its PowerShell command explicitly; for the
// other hosts the shell results are reported, since CI cannot observe a host's choice.
test('registered hook commands run through host shells', t => {
  const shells = windows
    ? { bash: command => ['bash', ['-c', command]], cmd: command => ['cmd.exe', ['/d', '/s', '/c', `"${command}"`]],
      powershell: command => ['powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', command]] }
    : { sh: command => ['sh', ['-c', command]] };
  const required = { claude: windows ? ['bash'] : ['sh'], copilot: windows ? ['bash', 'powershell'] : ['sh'] };
  const results = [];
  for (const host of ['claude', 'codex', 'cursor', 'copilot', 'gemini']) {
    const { root, project } = exported(t, host);
    const workspace = !['claude', 'codex'].includes(host);
    // Workspace exports are merged into the project; their commands run from its root.
    const cwd = workspace ? root : project;
    if (workspace) fs.cpSync(path.join(project, '.craft'), path.join(root, '.craft'), { recursive: true });
    const packageRoot = hookPackage(root, host, 'craft-review');
    const config = workspace ? read(path.join(root, { cursor: '.cursor/hooks.json', copilot: '.github/hooks/craft-capabilities.json', gemini: '.gemini/settings.json' }[host]))
      : read(path.join(packageRoot, 'platform-hooks/hooks.json'));
    const entries = Object.values(config.hooks).flat().flatMap(group => group.hooks ?? [group]);
    const entry = entries.find(hook => /--handler review-read-only/.test(hook.command ?? hook.bash));
    const variants = host === 'copilot' ? { bash: entry.bash, powershell: entry.powershell } : { any: entry.command };
    for (const [field, text] of Object.entries(variants)) {
      const command = text.replaceAll('${CLAUDE_PLUGIN_ROOT}', packageRoot).replaceAll('${PLUGIN_ROOT}', packageRoot);
      for (const [shell, form] of Object.entries(shells)) {
        if (field !== 'any' && (field === 'powershell') !== (shell === 'powershell')) continue;
        const [file, args] = form(command);
        const result = spawnSync(file, args, { cwd, env: { ...process.env, CLAUDE_PLUGIN_ROOT: packageRoot, PLUGIN_ROOT: packageRoot }, input: publish(cwd), encoding: 'utf8', windowsVerbatimArguments: shell === 'cmd' });
        const output = result.stdout.trim() ? (() => { try { return JSON.parse(result.stdout); } catch { return {}; } })() : {};
        const denied = host === 'claude' ? result.status === 2 && /read-only review/.test(result.stderr)
          : (output.hookSpecificOutput?.permissionDecision ?? output.permissionDecision ?? output.permission ?? output.decision) === 'deny';
        results.push({ host, field, shell, denied, status: result.status, stderr: result.stderr.trim().slice(0, 160) });
        if (required[host]?.includes(shell)) assert.ok(denied, `${host} ${field} via ${shell}: ${result.status} ${result.stdout} ${result.stderr}`);
      }
    }
  }
  t.diagnostic(`hook shells: ${JSON.stringify(results.map(({ host, field, shell, denied, status, stderr }) => `${host}/${field}/${shell}: ${denied ? 'ok' : `failed ${status} ${stderr}`}`))}`);
  if (!windows) assert.ok(results.every(result => result.denied), JSON.stringify(results));
});
