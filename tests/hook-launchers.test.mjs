import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { nativeTargets } from '../tooling/native-release.mjs';
import { hookPackage, runHook } from './hook-command.mjs';
import { launch } from './craft.mjs';
import { HOSTS } from '../hosts/registry.mjs';

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

// Host shells start the bundled runtime themselves; the launcher is only their fallback. Shells
// that name the platform (Bash, incl. Git Bash and macOS sh; zsh; PowerShell) skip it; others,
// such as dash, still run it. A runtime that crashes is not started a second time.
test('registered hook commands start the bundled runtime without the launcher', { skip: !current && 'no artifact for this platform' }, t => {
  const shell = file => command => [file, ['-c', command]];
  const ps = command => ['powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', command]];
  const names = file => spawnSync(file, ['-c', 'printf %s "$OSTYPE"'], { encoding: 'utf8' }).stdout !== '';
  const plans = windows ? { claude: [['command', shell('bash'), true]], codex: [['commandWindows', ps, true]], copilot: [['bash', shell('bash'), true], ['powershell', ps, true]] }
    : Object.fromEntries(['claude', 'codex', 'cursor', 'copilot'].map(host => [host, [[host === 'copilot' ? 'bash' : 'command', shell('sh'), names('sh')], [host === 'copilot' ? 'bash' : 'command', shell('bash'), true]]]));
  for (const [host, plan] of Object.entries(plans)) {
    const { root, project } = exported(t, host);
    const workspace = !['claude', 'codex'].includes(host);
    const cwd = workspace ? root : project;
    if (workspace) fs.cpSync(path.join(project, '.craft'), path.join(root, '.craft'), { recursive: true });
    const packageRoot = hookPackage(root, host, 'craft-review');
    assemble(packageRoot);
    const runtime = path.join(packageRoot, 'support/native-bin', current.target, 'native-bin', `craft${windows ? '.exe' : ''}`);
    if (!windows) fs.chmodSync(runtime, 0o755);
    const config = read(workspace ? path.join(root, { cursor: '.cursor/hooks.json', copilot: '.github/hooks/craft-capabilities.json' }[host]) : path.join(packageRoot, 'platform-hooks/hooks.json'));
    const entry = Object.values(config.hooks).flat().flatMap(group => group.hooks ?? [group]).find(hook => /--handler review-read-only/.test(hook.command ?? hook.bash));
    const env = { ...plain, CLAUDE_PLUGIN_ROOT: packageRoot, PLUGIN_ROOT: packageRoot };
    const run = (field, form) => {
      const [file, args] = form(host === 'claude' ? entry[field].replaceAll('${CLAUDE_PLUGIN_ROOT}', packageRoot) : entry[field]);
      return spawnSync(file, args, { cwd, env, input: publish(cwd), encoding: 'utf8' });
    };
    const decision = result => host === 'claude' ? (result.status === 2 ? 'deny' : 'allow')
      : (({ hookSpecificOutput, permissionDecision, permission } = {}) => hookSpecificOutput?.permissionDecision ?? permissionDecision ?? permission)(JSON.parse(result.stdout || '{}'));
    const reason = result => host === 'claude' ? result.stderr : result.stdout;
    // A launcher that only records being started.
    const marker = path.join(root, `${host}-launcher-started`);
    const launchers = ['bin/craft', 'bin/craft.cmd'].map(name => [path.join(packageRoot, name), fs.readFileSync(path.join(packageRoot, name))]);
    fs.writeFileSync(launchers[0][0], `#!/bin/sh\necho started >> "${marker}"\nexit 1\n`);
    fs.writeFileSync(launchers[1][0], `@echo started>>"${marker}"\r\nexit /b 1\r\n`);
    for (const [field, form, direct] of plan) {
      fs.rmSync(marker, { force: true });
      const result = run(field, form);
      const label = `${host} ${field} via ${form('')[0]}`;
      if (!direct) { assert.ok(fs.existsSync(marker), `${label}: expected the launcher`); continue; }
      assert.ok(!fs.existsSync(marker), `${label}: started the launcher`);
      assert.equal(decision(result), 'deny', `${label}: ${result.status} ${result.stdout} ${result.stderr}`);
      assert.match(reason(result), /read-only review/);
    }
    for (const [file, text] of launchers) fs.writeFileSync(file, text);
    if (windows) continue;
    // A runtime that crashes runs once; the launcher then keeps shell policy closed.
    const count = path.join(root, `${host}-runtime-started`);
    fs.writeFileSync(runtime, `#!/bin/sh\necho started >> "${count}"\ncat > /dev/null\nexit 3\n`);
    for (const [field, form] of plan) {
      fs.rmSync(count, { force: true });
      const result = run(field, form);
      assert.equal(fs.readFileSync(count, 'utf8'), 'started\n', `${host} ${field}: runtime runs once`);
      assert.equal(decision(result), 'deny', `${host} ${field}: ${result.status} ${result.stdout} ${result.stderr}`);
      assert.match(reason(result), /Policy could not be evaluated: bundled native runtime unavailable/);
    }
  }
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
    // Cursor blocks a permission hook that answers outside its schema; a silent hook says `allow`.
    assert.deepEqual([observed.status, observed.stdout.trim(), observed.stderr], [0, host === 'cursor' ? '{"permission":"allow"}' : '', '']);
    // A tool call the fallback does not deny gets no grant where the host takes none.
    const routed = run('craft-delivery', 'routing', publish(project));
    assert.equal(routed.status, 0, routed.stderr);
    if (host === 'cursor') assert.deepEqual(JSON.parse(routed.stdout), { permission: 'allow' });
    else assert.equal(routed.stdout, '', `${host}: ${routed.stdout}`);
    // Every package says once, when the session starts, that it is inactive; the person sees
    // it where the host shows a session-start notice, otherwise the model is asked to say so.
    const notice = HOSTS[host].events['session-start'].notice === true;
    for (const [name, hook] of [['craft-delivery', 'restore'], ['craft-engineering', 'observe-session-start']]) {
      const started = run(name, hook, JSON.stringify({ session_id: 'S-1', cwd: project }));
      assert.equal(started.status, 0, started.stderr);
      const output = JSON.parse(started.stdout);
      const context = output.hookSpecificOutput?.additionalContext ?? output.additionalContext ?? output.additional_context;
      assert.match(context, new RegExp(`^${name} is inactive: its bundled native runtime is missing or cannot start here`), started.stdout);
      assert.equal(output.systemMessage?.startsWith(`${name} is inactive`) ?? false, notice, started.stdout);
      assert.equal(/Tell the user once/.test(context), !notice, context);
    }
    // Later hooks stay quiet instead of repeating it on every prompt and tool call.
    const prompted = run('craft-delivery', 'prompt-routing', JSON.stringify({ session_id: 'S-1', cwd: project, prompt: 'implement it' }));
    assert.equal(prompted.status, 0, prompted.stderr);
    assert.doesNotMatch(prompted.stdout, /inactive|unavailable/);
    // A command a person or skill runs fails explicitly instead.
    const command = launch(pkg('craft-delivery'), ['route', '--root', project, '--intent', 'implement'], { cwd: project, env: plain });
    assert.deepEqual([command.status, command.stdout], [1, '']);
    assert.match(command.stderr, /^craft: bundled native runtime unavailable\r?\n$/);
    assert.ok(!fs.existsSync(path.join(project, '.craft/state')), 'no runtime ran');
  }
});

// Each registered command string as its host runs it: Claude substitutes its plugin root and
// uses Bash (Git Bash on Windows); Codex sets PLUGIN_ROOT only in the environment and uses the
// session shell (PowerShell on Windows, with `commandWindows`; cmd without a session shell);
// Gemini uses bash, or PowerShell on Windows with an exit-code suffix; Copilot names both.
// Cursor's Windows shell is not documented, so its results there are reported only.
// Enforcement must come from the runtime's policy, not the unavailable-runtime fallback.
test('registered hook commands run through host shells', t => {
  const pwsh = windows && spawnSync('pwsh', ['-NoProfile', '-Command', 'exit 0']).status === 0;
  const powershell = (suffix = '') => Object.fromEntries([['powershell', 'powershell.exe'], ...(pwsh ? [['pwsh', 'pwsh']] : [])]
    .map(([name, file]) => [name, command => [file, ['-NoProfile', '-NonInteractive', '-Command', `${command}${suffix}`]]]));
  const bash = { bash: command => ['bash', ['-c', command]] };
  const sh = { sh: command => ['sh', ['-c', command]] };
  const cmd = { cmd: command => ['cmd.exe', ['/C', `"${command}"`]] };
  const plans = windows ? {
    claude: [{ field: 'command', required: bash }],
    codex: [{ field: 'commandWindows', required: powershell(), reported: cmd }],
    cursor: [{ field: 'command', reported: { ...bash, ...powershell() } }],
    copilot: [{ field: 'bash', required: bash }, { field: 'powershell', required: powershell() }],
    gemini: [{ field: 'command', required: powershell('; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }') }]
  } : {
    claude: [{ field: 'command', required: sh }], codex: [{ field: 'command', required: sh }], cursor: [{ field: 'command', required: sh }],
    copilot: [{ field: 'bash', required: sh }], gemini: [{ field: 'command', required: bash }]
  };
  const results = [];
  for (const [host, plan] of Object.entries(plans)) {
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
    for (const { field, required = {}, reported = {} } of plan) {
      const text = entry[field];
      assert.equal(typeof text, 'string', `${host} registers ${field}`);
      const command = host === 'claude' ? text.replaceAll('${CLAUDE_PLUGIN_ROOT}', packageRoot) : text;
      for (const [shell, form] of Object.entries({ ...required, ...reported })) {
        const [file, args] = form(command);
        const result = spawnSync(file, args, { cwd, env: { ...process.env, CLAUDE_PLUGIN_ROOT: packageRoot, PLUGIN_ROOT: packageRoot }, input: publish(cwd), encoding: 'utf8', windowsVerbatimArguments: shell === 'cmd' });
        const output = result.stdout?.trim() ? (() => { try { return JSON.parse(result.stdout); } catch { return {}; } })() : {};
        const reason = host === 'claude' ? result.stderr ?? '' : JSON.stringify(output);
        const denied = (host === 'claude' ? result.status === 2
          : (output.hookSpecificOutput?.permissionDecision ?? output.permissionDecision ?? output.permission ?? output.decision) === 'deny')
          && /read-only review/.test(reason) && !/could not be evaluated/.test(reason);
        results.push({ host, field, shell, denied, status: result.status, stderr: `${result.stderr ?? result.error?.message ?? ''}`.trim().slice(0, 160) });
        if (shell in required) assert.ok(denied, `${host} ${field} via ${shell}: ${result.status} ${result.stdout} ${result.stderr}`);
      }
    }
  }
  t.diagnostic(`hook shells: ${JSON.stringify(results.map(({ host, field, shell, denied, status, stderr }) => `${host}/${field}/${shell}: ${denied ? 'ok' : `failed ${status} ${stderr}`}`))}`);
});
