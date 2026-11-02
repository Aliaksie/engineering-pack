// Installed-host smoke: every host's registered hook command, exactly as its export writes it,
// run from its installed layout under each shell that host uses on this OS. Each command must
// answer like the bundled runtime called directly; Hyperfine then times it, and each shell's own
// start time is measured separately because Hyperfine subtracts it from a `--shell` command.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { bencherMetrics } from './bencher-metrics.mjs';
import { consumer, directoryBytes, gitBash, measurer, quote, run, slash, version, windows } from './fixture.mjs';

const usage = 'usage: hosts.mjs --dist BUILT_DIST --output NEW_DIRECTORY [--runs 20] [--hyperfine PATH] [--bash PATH]';
const options = { runs: '20', hyperfine: 'hyperfine' };
for (let i = 2; i < process.argv.length; i += 2) {
  const key = process.argv[i]?.replace(/^--/, '');
  if (!['dist', 'output', 'runs', 'hyperfine', 'bash'].includes(key) || process.argv[i + 1] === undefined) throw new Error(usage);
  options[key] = process.argv[i + 1];
}
const runs = Number(options.runs);
if (!options.dist || !options.output || !(runs > 0) || fs.existsSync(options.output)) throw new Error(usage);
const dist = path.resolve(options.dist), output = path.resolve(options.output);
const benchmarks = path.dirname(fileURLToPath(import.meta.url));
fs.mkdirSync(path.join(output, 'raw'), { recursive: true });

// Shells present on this machine, as hosts start them: `<program> <args> -c <command>`.
const available = ({ program, args }) => spawnSync(program, [...args, '-c', 'exit 0'], { stdio: 'ignore' }).status === 0;
const posix = windows ? [] : [{ name: 'sh', program: '/bin/sh', args: [] }, { name: 'bash', program: 'bash', args: [] }, { name: 'zsh', program: 'zsh', args: [] }];
const powershell = windows ? [{ name: 'powershell', program: 'powershell', args: ['-NoProfile'] }, { name: 'pwsh', program: 'pwsh', args: ['-NoProfile'] }] : [];
const bash = [{ name: 'git-bash', program: options.bash ?? gitBash(), args: [] }];
const present = list => list.filter(available);

// Which field of a registered hook each host runs, and in which shells, per OS.
const FORMS = [
  { host: 'claude', field: 'command', shells: windows ? bash : posix },
  { host: 'codex', field: windows ? 'commandWindows' : 'command', shells: windows ? powershell : posix },
  // Cursor does not document its Windows hook shell; Git Bash there is an assumption.
  { host: 'cursor', field: 'command', shells: windows ? bash : posix },
  { host: 'copilot', field: windows ? 'powershell' : 'bash', shells: windows ? powershell : posix },
  { host: 'gemini', field: 'command', shells: windows ? powershell : posix },
];
const CONFIG = {
  claude: 'plugins/craft-delivery/platform-hooks/hooks.json', codex: 'packages/craft-delivery/platform-hooks/hooks.json',
  cursor: '.cursor/hooks.json', copilot: '.github/hooks/craft-capabilities.json', gemini: '.gemini/settings.json',
};
const PLUGIN = { claude: 'plugins/craft-delivery', codex: 'packages/craft-delivery' };
const EVENTS = [{ cell: 'prompt', event: 'before-prompt' }, { cell: 'tool-shell', event: 'before-tool' }, { cell: 'stop', event: 'turn-end' }];

// Each host's own session and tool fields for one lifecycle event.
function payload(host, event, cwd) {
  const id = host === 'cursor' ? { conversation_id: 'smoke-session' } : host === 'copilot' ? { sessionId: 'smoke-session' } : { session_id: 'smoke-session' };
  const base = { ...id, cwd: slash(cwd) };
  if (event === 'before-prompt') return { ...base, prompt: 'Implement a guard in src/item-0.mjs and add a test' };
  if (event === 'turn-end') return base;
  if (host === 'copilot') return { ...base, toolName: 'bash', toolArgs: { command: 'git status --short' } };
  return { ...base, tool_name: { claude: 'Bash', codex: 'Bash', cursor: 'Shell', gemini: 'run_shell_command' }[host], tool_input: { command: 'git status --short' } };
}

// The craft-delivery routing command a host registered for one event, in one field.
function registered(host, field, event) {
  const config = JSON.parse(fs.readFileSync(path.join(dist, host, CONFIG[host]), 'utf8'));
  const texts = Object.values(config.hooks ?? {}).flat().flatMap(group => group.hooks ?? [group]).map(hook => hook[field])
    .filter(text => text?.includes(` hook ${host} ${event} --package craft-delivery --handler deliver `));
  if (texts.length > 1) throw new Error(`${host}: ${texts.length} ${field} commands for ${event}`);
  return texts[0];
}

const workspace = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-hosts-'));
const home = path.join(workspace, 'home');
fs.mkdirSync(home);
const base = consumer(path.join(workspace, 'consumer'));
const native = (() => {
  const bundles = path.join(dist, 'claude', PLUGIN.claude, 'support/native-bin');
  const targets = fs.readdirSync(bundles, { withFileTypes: true }).filter(entry => entry.isDirectory()).map(entry => entry.name);
  if (targets.length !== 1) throw new Error(`expected one installed native target, found ${targets.length}`);
  return { target: targets[0], file: path.join(bundles, targets[0], 'native-bin', windows ? 'craft.exe' : 'craft') };
})();

// Per host: an installed layout and its own consumer copy with seeded state. Overlay hosts install
// into the project, ignored by Git as a consumer that does not commit them would.
const installs = Object.fromEntries(FORMS.map(({ host }) => host).filter((host, i, all) => all.indexOf(host) === i).map(host => {
  const root = path.join(workspace, host, 'project');
  fs.cpSync(base, root, { recursive: true });
  let packageRoot, env = { ...process.env, HOME: home, USERPROFILE: home };
  if (PLUGIN[host]) {
    packageRoot = path.join(dist, host, PLUGIN[host]);
    env = host === 'claude' ? { ...env, CLAUDE_PLUGIN_ROOT: slash(packageRoot), CLAUDE_PROJECT_DIR: slash(root) } : { ...env, PLUGIN_ROOT: slash(packageRoot) };
  } else {
    const entries = fs.readdirSync(path.join(dist, host));
    for (const entry of entries) fs.cpSync(path.join(dist, host, entry), path.join(root, entry), { recursive: true });
    fs.appendFileSync(path.join(root, '.git/info/exclude'), entries.map(entry => `/${entry}\n`).join(''));
    packageRoot = path.join(root, 'packages/craft-delivery');
  }
  const payloads = Object.fromEntries(EVENTS.map(({ cell, event }) => {
    const file = path.join(workspace, host, `${cell}.json`);
    fs.writeFileSync(file, JSON.stringify(payload(host, event, root)) + '\n');
    return [cell, file];
  }));
  const args = event => ['hook', host, event, '--package', 'craft-delivery', '--handler', 'deliver', '--scope', 'any'];
  const direct = (cell, event) => spawnSync(native.file, args(event), { cwd: root, env: { ...env, CRAFT_PACKAGE_ROOT: packageRoot }, input: fs.readFileSync(payloads[cell]), encoding: 'utf8' });
  direct('prompt', 'before-prompt');
  const seed = path.join(workspace, host, 'seed');
  fs.cpSync(path.join(root, '.craft'), seed, { recursive: true });
  const state = [path.join(benchmarks, 'reset-state.mjs'), seed, path.join(root, '.craft')];
  return [host, { root, env, payloads, direct, reset: `node ${state.map(quote).join(' ')}`, restore: () => run(process.execPath, state) }];
}));

const shellsUsed = present([...posix, ...powershell, ...(windows ? bash : [])]);
const results = { formatVersion: 1, hyperfine: version(options.hyperfine, ['--version']),
  platform: { os: process.platform, arch: process.arch, release: os.release(), cpu: os.cpus()[0]?.model ?? null, logicalCpus: os.cpus().length,
    runner: Object.fromEntries(['RUNNER_OS', 'RUNNER_ARCH', 'ImageOS', 'ImageVersion', 'GITHUB_RUN_ID', 'GITHUB_RUN_ATTEMPT']
      .filter(key => process.env[key] !== undefined).map(key => [key, process.env[key]])) },
  shells: Object.fromEntries([...posix, ...powershell, ...(windows ? bash : [])].map(shell => [shell.name,
    shellsUsed.includes(shell) ? version(shell.program, [...shell.args, '-c', shell.name.includes('powershell') || shell.name === 'pwsh' ? '$PSVersionTable.PSVersion.ToString()' : 'echo "${BASH_VERSION:-${ZSH_VERSION:-sh}}"']) : null])),
  runs, runtime: { target: native.target, nativeBytes: fs.statSync(native.file).size, pluginBytes: directoryBytes(path.join(dist, 'claude', PLUGIN.claude)) },
  cells: [], checks: [], skipped: [], failures: [] };
const save = () => fs.writeFileSync(path.join(output, 'results.json'), JSON.stringify(results, null, 2) + '\n');
const measure = measurer({ hyperfine: options.hyperfine, output, results, save });
const shellLine = shell => [shell.program, ...shell.args].map(part => part.includes(' ') ? `"${slash(part)}"` : slash(part)).join(' ');

for (const shell of shellsUsed) {
  measure(`shell/${shell.name}/start`, workspace, ['-N', '--warmup', '3', '--runs', String(runs), `${shellLine(shell)} -c exit`]);
}
for (const { host, field, shells } of FORMS) {
  const install = installs[host];
  for (const shell of shells) {
    const label = `hosts/${host}/${field}/${shell.name}`;
    if (!shellsUsed.includes(shell)) { results.skipped.push({ cell: label, reason: 'shell not installed' }); continue; }
    for (const { cell, event } of EVENTS) {
      const text = registered(host, field, event);
      if (!text) { results.skipped.push({ cell: `${label}/${cell}`, reason: 'no registered command for this event' }); continue; }
      const command = host === 'claude' ? text.replaceAll('${CLAUDE_PLUGIN_ROOT}', install.env.CLAUDE_PLUGIN_ROOT) : text;
      // Outside the timed runs: the registered command answers exactly as the runtime does.
      install.restore();
      const hosted = spawnSync(shell.program, [...shell.args, '-c', command], { cwd: install.root, env: install.env, input: fs.readFileSync(install.payloads[cell]), encoding: 'utf8' });
      install.restore();
      const expected = install.direct(cell, event);
      const same = hosted.status === expected.status && hosted.stdout === expected.stdout && !hosted.stderr;
      results.checks.push({ cell: `${label}/${cell}`, status: hosted.status, expectedStatus: expected.status, stdout: hosted.stdout, stderr: hosted.stderr, same });
      if (!same) { results.failures.push(`${label}/${cell}: answer differs from the runtime (exit ${hosted.status}, expected ${expected.status})`); continue; }
      measure(`${label}/${cell}`, install.root, ['--shell', shellLine(shell), '--warmup', '3', '--runs', String(runs),
        '--prepare', install.reset,
        '--input', slash(install.payloads[cell]), command], install.env);
    }
  }
}
save();
fs.writeFileSync(path.join(output, 'bencher.json'), JSON.stringify(bencherMetrics(results, file => JSON.parse(fs.readFileSync(path.join(output, file), 'utf8'))), null, 2) + '\n');
fs.rmSync(workspace, { recursive: true, force: true });
for (const failure of results.failures) console.error(`failed: ${failure}`);
if (results.failures.length) process.exitCode = 1;
