// Measure an installed Claude export with Hyperfine: startup, hooks, Project Intelligence refresh and,
// per supported language, extraction and one structural lookup over a generated fixture.
// Hooks run in three layers: the native binary, the package launcher, and the registered hook
// command under the shell hosts use. Writes raw Hyperfine JSON, checked outputs and Bencher Metric Format results.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { bencherMetrics } from './bencher-metrics.mjs';
import { consumer, directoryBytes, git, gitBash, measurer, quote, run, slash, version, windows } from './fixture.mjs';
import { LANGUAGES, PROBE, languageFixture } from './languages.mjs';

const usage = 'usage: suite.mjs --export CLAUDE_EXPORT --fixture SOURCE_CHECKOUT --output NEW_DIRECTORY [--runs 30] [--cold-runs 10] [--hyperfine PATH] [--bash PATH]';
const options = { runs: '30', 'cold-runs': '10', hyperfine: 'hyperfine' };
for (let i = 2; i < process.argv.length; i += 2) {
  const key = process.argv[i]?.replace(/^--/, '');
  if (!['export', 'fixture', 'output', 'runs', 'cold-runs', 'hyperfine', 'bash'].includes(key) || process.argv[i + 1] === undefined) throw new Error(usage);
  options[key] = process.argv[i + 1];
}
const runs = Number(options.runs), coldRuns = Number(options['cold-runs']);
if (!options.export || !options.fixture || !options.output || !(runs > 0) || !(coldRuns > 0) || fs.existsSync(options.output)) throw new Error(usage);

const benchmarks = path.dirname(fileURLToPath(import.meta.url));
const output = path.resolve(options.output), raw = path.join(output, 'raw');
fs.mkdirSync(raw, { recursive: true });

// Hosts start hooks through bash; on Windows that is Git Bash, never System32's WSL bash.
const bash = options.bash ?? gitBash();

const HOOKS = [
  { cell: 'prompt', event: 'before-prompt', payload: { hook_event_name: 'UserPromptSubmit', prompt: 'Implement a guard in src/item-0.mjs and add a test' } },
  { cell: 'prompt-routed', event: 'before-prompt', payload: { hook_event_name: 'UserPromptSubmit', prompt: 'Review the guard in src/item-0.mjs and its test' } },
  { cell: 'tool-read', event: 'before-tool', payload: root => ({ hook_event_name: 'PreToolUse', tool_name: 'Read', tool_input: { file_path: slash(path.join(root, 'src/item-0.mjs')) }, tool_use_id: 'toolu_read' }) },
  { cell: 'tool-edit', event: 'before-tool', payload: root => ({ hook_event_name: 'PreToolUse', tool_name: 'Edit', tool_input: { file_path: slash(path.join(root, 'src/item-0.mjs')), old_string: '1', new_string: '2' }, tool_use_id: 'toolu_edit' }) },
  { cell: 'stop', event: 'turn-end', payload: { hook_event_name: 'Stop', stop_hook_active: false } }
];
const hookArguments = event => `hook claude ${event} --package craft-delivery --handler deliver --scope any`;

const plugin = path.resolve(options.export, 'plugins/craft-delivery');
const targets = fs.readdirSync(path.join(plugin, 'support/native-bin'), { withFileTypes: true }).filter(entry => entry.isDirectory()).map(entry => entry.name);
if (targets.length !== 1) throw new Error(`expected one installed native target, found ${targets.length}`);
const bundle = path.join(plugin, 'support/native-bin', targets[0]);
const native = path.join(bundle, 'native-bin', windows ? 'craft.exe' : 'craft');
const manifest = JSON.parse(fs.readFileSync(path.join(bundle, 'native-bundle.json'), 'utf8'));

const workspace = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-performance-'));
const root = consumer(path.join(workspace, 'consumer')), home = path.join(workspace, 'home'), seed = path.join(workspace, 'seed');
fs.mkdirSync(home);
const env = { ...process.env, HOME: home, USERPROFILE: home, CLAUDE_PLUGIN_ROOT: slash(plugin), CLAUDE_PROJECT_DIR: slash(root) };
const payloads = Object.fromEntries(HOOKS.map(({ cell, payload }) => {
  const file = path.join(workspace, `${cell}.json`);
  const body = typeof payload === 'function' ? payload(root) : payload;
  fs.writeFileSync(file, JSON.stringify({ session_id: 'bench-session', transcript_path: slash(path.join(home, 'transcript.jsonl')), cwd: slash(root), ...body }) + '\n');
  return [cell, file];
}));
// Seed state after one prompt so every measured run starts from identical .craft content.
run(native, hookArguments('before-prompt').split(' '), { cwd: root, env, input: fs.readFileSync(payloads.prompt) });
fs.cpSync(path.join(root, '.craft'), seed, { recursive: true });
const restore = () => run(process.execPath, [path.join(benchmarks, 'reset-state.mjs'), seed, path.join(root, '.craft')]);

// Project Intelligence fixture: a clean copy of the source checkout at HEAD.
const project = path.join(workspace, 'project'), archive = path.join(workspace, 'project.tar');
git(options.fixture, 'archive', '--format=tar', '-o', archive, 'HEAD');
fs.mkdirSync(project);
// A relative archive path: Git Bash's GNU tar reads `C:` in an absolute path as a remote host.
run('tar', ['-xf', path.relative(project, archive)], { cwd: project });
git(project, 'init', '-q'); git(project, 'add', '.'); git(project, 'commit', '-qm', 'fixture');
const refreshAt = root => JSON.parse(run(native, ['project-evidence', 'refresh', '--enable', '--root', slash(root), '--json'], { env }));
const firstRefresh = refreshAt(project);
// One untimed warm refresh, for where its time goes (Git inventory, file metadata, reading, store).
const warmRefresh = refreshAt(project);
const languageRoot = language => path.join(workspace, 'languages', language);
const languages = Object.fromEntries(Object.entries(LANGUAGES).map(([language, { level }]) => {
  const refreshed = refreshAt(languageFixture(languageRoot(language), language));
  return [language, { level, files: refreshed.files, failed: refreshed.failed, bytesScanned: refreshed.bytesScanned,
    phasesMs: refreshed.phasesMs, wallTimeMs: refreshed.wallTimeMs }];
}));

const results = { formatVersion: 1, hyperfine: version(options.hyperfine, ['--version']),
  platform: { os: process.platform, arch: process.arch, release: os.release(), cpu: os.cpus()[0]?.model ?? null, logicalCpus: os.cpus().length,
    memoryBytes: os.totalmem(), runner: Object.fromEntries(['RUNNER_OS', 'RUNNER_ARCH', 'ImageOS', 'ImageVersion', 'GITHUB_RUN_ID', 'GITHUB_RUN_ATTEMPT']
      .filter(key => process.env[key] !== undefined).map(key => [key, process.env[key]])) },
  tools: { node: process.version, git: version('git', ['--version']), gitPath: version(windows ? 'where' : 'which', ['git']), bash: version(bash, ['--version']) },
  runs, coldRuns, fixtureRevision: git(options.fixture, 'rev-parse', 'HEAD').trim(),
  runtime: { target: targets[0], sourceRevision: manifest.sourceRevision ?? null,
    nativeSha256: createHash('sha256').update(fs.readFileSync(native)).digest('hex'), nativeBytes: fs.statSync(native).size, pluginBytes: directoryBytes(plugin) },
  projectIntelligence: { files: firstRefresh.files, failed: firstRefresh.failed, evidence: firstRefresh.coverage?.evidence, bytesScanned: firstRefresh.bytesScanned,
    phasesMs: { cold: firstRefresh.phasesMs, warm: warmRefresh.phasesMs }, wallTimeMs: { cold: firstRefresh.wallTimeMs, warm: warmRefresh.wallTimeMs }, languages },
  cells: [], checks: [], failures: [] };
const save = () => fs.writeFileSync(path.join(output, 'results.json'), JSON.stringify(results, null, 2) + '\n');


const measure = measurer({ hyperfine: options.hyperfine, output, results, save });

// The command the plugin registers for this hook, as Claude runs it: its root substituted, under bash.
const hookConfig = JSON.parse(fs.readFileSync(path.join(plugin, 'platform-hooks/hooks.json'), 'utf8'));
const registered = event => {
  const commands = Object.values(hookConfig.hooks).flat().flatMap(group => group.hooks).map(hook => hook.command)
    .filter(command => command.endsWith(` ${hookArguments(event)}`));
  if (commands.length !== 1) throw new Error(`expected one registered ${event} hook, found ${commands.length}`);
  return commands[0].replaceAll('${CLAUDE_PLUGIN_ROOT}', slash(plugin));
};
const reset = ['--prepare', `node ${quote(path.join(benchmarks, 'reset-state.mjs'))} ${quote(seed)} ${quote(path.join(root, '.craft'))}`];
measure('startup/native', root, ['-N', '--warmup', '3', '--runs', String(runs), `${quote(native)} --help`]);
// One snapshot query through the Git on PATH and, on Windows, through the Git executable the
// Git for Windows PATH entry forwards to: whether each Git call pays for a second process start.
const statusQuery = 'status --porcelain=v1 -z --untracked-files=all';
const gits = { path: 'git', ...(windows ? { direct: path.resolve(run('git', ['--exec-path']).trim(), '../../bin/git.exe') } : {}) };
for (const [layer, program] of Object.entries(gits)) {
  measure(`git/status/${layer}`, root, ['-N', '--warmup', '3', '--runs', String(runs), `${layer === 'path' ? program : quote(program)} -C ${quote(root)} ${statusQuery}`]);
}
for (const { cell, event } of HOOKS) {
  const layers = {
    native: ['-N', `${quote(native)} ${hookArguments(event)}`],
    launcher: ['-N', `${quote(path.join(plugin, windows ? 'bin/craft.cmd' : 'bin/craft'))} ${hookArguments(event)}`],
    host: ['--shell', windows ? quote(bash) : bash, registered(event)]
  };
  for (const [layer, command] of Object.entries(layers)) {
    measure(`hook/${cell}/${layer}`, root, ['--warmup', '3', '--runs', String(runs), ...reset, '--input', slash(payloads[cell]), ...command]);
  }
}
const refresh = `${quote(native)} project-evidence refresh --enable --root ${quote(project)} --json`;
measure('project-intelligence/refresh-warm/native', project, ['-N', '--warmup', '2', '--runs', String(runs), refresh]);
// cmd.exe /C strips the outer quotes of a prepare line that starts with one: leave the binary unquoted.
measure('project-intelligence/refresh-cold/native', project, ['-N', '--runs', String(coldRuns),
  '--prepare', `${slash(native)} project-evidence rebuild --enable --root ${quote(project)} --json`, refresh]);
// Per language: cold extraction of the generated fixture and one declaration lookup on the warm index.
const declaration = root => `${quote(native)} locate --root ${quote(root)} --declaration ${PROBE.name} --json`;
for (const language of Object.keys(LANGUAGES)) {
  const root = languageRoot(language), refreshCommand = `${quote(native)} project-evidence refresh --enable --root ${quote(root)} --json`;
  measure(`project-intelligence/language/${language}/refresh-cold/native`, root, ['-N', '--runs', String(coldRuns),
    '--prepare', `${slash(native)} project-evidence rebuild --enable --root ${quote(root)} --json`, refreshCommand]);
  measure(`project-intelligence/language/${language}/declaration/native`, root, ['-N', '--warmup', '2', '--runs', String(runs), declaration(root)]);
}

// Correctness, outside the timed runs: each hook exits 0 without stderr and answers with JSON or nothing.
for (const { cell, event } of HOOKS) {
  restore();
  const result = spawnSync(native, hookArguments(event).split(' '), { cwd: root, env, input: fs.readFileSync(payloads[cell]), encoding: 'utf8' });
  let valid = result.stdout === '';
  try { valid ||= JSON.parse(result.stdout) !== null; } catch { valid = false; }
  results.checks.push({ cell, status: result.status, stderr: result.stderr, stdout: result.stdout });
  if (result.status !== 0 || result.stderr || !valid) results.failures.push(`hook/${cell}: output check failed (exit ${result.status})`);
}
const status = JSON.parse(run(native, ['project-evidence', 'status', '--enable', '--root', slash(project), '--json'], { env }));
results.checks.push({ cell: 'project-intelligence/status', sources: status.sources, failedSources: status.failedSources, inventoryComplete: status.inventoryComplete });
if (!(status.sources > 0) || status.failedSources !== 0 || status.inventoryComplete !== true) results.failures.push('project-intelligence: incomplete refresh');
// Each language fixture is read without failure, and its index declares the probe and finds its one caller.
for (const [language, { file }] of Object.entries(LANGUAGES)) {
  const root = languageRoot(language), ask = (...args) => JSON.parse(run(native, ['locate', '--root', slash(root), ...args, '--json'], { env }));
  const found = ask('--declaration', PROBE.name), callers = ask('--callers', PROBE.name);
  const check = { cell: `project-intelligence/language/${language}`, failed: languages[language].failed,
    declaredIn: found.results?.map(row => row.path), answeredBy: found.route?.answeredBy,
    calledFrom: callers.results?.map(row => row.container?.name ?? null) };
  results.checks.push(check);
  if (check.failed !== 0 || check.answeredBy !== 'intelligence' || check.declaredIn?.join() !== file(PROBE.file)
    || check.calledFrom?.join() !== PROBE.caller) results.failures.push(`project-intelligence/language/${language}: lookup check failed`);
}
save();

const metrics = bencherMetrics(results, file => JSON.parse(fs.readFileSync(path.join(output, file), 'utf8')));
fs.writeFileSync(path.join(output, 'bencher.json'), JSON.stringify(metrics, null, 2) + '\n');
fs.rmSync(workspace, { recursive: true, force: true });
for (const failure of results.failures) console.error(`failed: ${failure}`);
if (results.failures.length) process.exitCode = 1;
