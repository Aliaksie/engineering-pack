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
import { bencherMetrics, testbed } from './bencher-metrics.mjs';
import { consumer, directoryBytes, git, gitBash, measurer, quote, run, slash, version, windows } from './fixture.mjs';
import { LANGUAGES, PROBE, languageFixture } from './languages.mjs';
import { peakMemory } from './peak-memory.mjs';

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

// Standards fixture: the consumer with declared standards and debt markers, one caller of
// `item42`, a started session, a routed change, a warm index and an uncommitted edit adding a
// TODO. Each cell is a hook the session then fires; its check says the feature answered.
const standards = consumer(path.join(workspace, 'standards')), standardsSeed = path.join(workspace, 'standards-seed');
fs.writeFileSync(path.join(standards, 'src/caller.mjs'), "import { item42 } from './item-42.mjs';\nexport const total = value => item42(value);\n");
fs.writeFileSync(path.join(standards, '.craft/project.json'), JSON.stringify({ formatVersion: 1, routing: { debtMarkers: true }, standards: [
  { id: 'commits', anchor: 'conventional-commits' },
  { id: 'item-review', checklist: ['Guards reject negative values'], scope: { paths: ['src'] } }] }) + '\n');
git(standards, 'add', '.'); git(standards, 'commit', '-qm', 'standards');
// The background warm-up session start would launch is measured as its own cell.
const standardsEnv = { ...env, CLAUDE_PROJECT_DIR: slash(standards), CRAFT_EVIDENCE_WARM: '0' };
const SESSION = [
  { cell: 'session-start', event: 'session-start', payload: { hook_event_name: 'SessionStart', source: 'startup' }, answers: out => out.includes('Craft session') },
  { cell: 'after-read', event: 'after-tool', payload: root => ({ hook_event_name: 'PostToolUse', tool_name: 'Read', tool_input: { file_path: slash(path.join(root, 'src/item-1.mjs')) }, tool_response: { type: 'text' }, tool_use_id: 'toolu_after_read' }), answers: () => true },
  { cell: 'after-search', event: 'after-tool', payload: root => ({ hook_event_name: 'PostToolUse', tool_name: 'Grep', tool_input: { pattern: 'item42', path: slash(root) }, tool_response: { mode: 'files_with_matches', filenames: [] }, tool_use_id: 'toolu_after_search' }), answers: out => out.includes('src/caller.mjs:2') },
  { cell: 'after-test', event: 'after-tool', payload: { hook_event_name: 'PostToolUse', tool_name: 'Bash', tool_input: { command: 'npm test' }, tool_response: { stdout: '✔ guards (1ms)\nℹ tests 1\nℹ pass 1\nℹ fail 0\nℹ skipped 0\n', stderr: '', interrupted: false }, tool_use_id: 'toolu_after_test' },
    answers: (_, root) => fs.readdirSync(path.join(root, '.craft/state/sessions'), { recursive: true }).some(file => String(file).endsWith('test-runs.json')) },
  { cell: 'agent-start', event: 'agent-start', payload: { hook_event_name: 'SubagentStart', agent_id: 'bench-agent', agent_type: 'general-purpose' }, answers: out => out.includes('conventional-commits') },
  { cell: 'stop-changed', event: 'turn-end', payload: { hook_event_name: 'Stop', stop_hook_active: false }, answers: out => out.includes('"block"') && out.includes('todo-marker') && out.includes('item-review') }
];
const payload = (root, cell, body) => {
  const file = path.join(workspace, `${cell}.json`);
  fs.writeFileSync(file, JSON.stringify({ session_id: 'bench-session', transcript_path: slash(path.join(home, 'transcript.jsonl')), cwd: slash(root), ...body }) + '\n');
  return file;
};
for (const step of SESSION) step.file = payload(standards, step.cell, typeof step.payload === 'function' ? step.payload(standards) : step.payload);
run(native, hookArguments('session-start').split(' '), { cwd: standards, env: standardsEnv, input: fs.readFileSync(SESSION[0].file) });
run(native, hookArguments('before-prompt').split(' '), { cwd: standards, env: standardsEnv, input: fs.readFileSync(payload(standards, 'standards-prompt', HOOKS[0].payload)) });
run(native, ['project-evidence', 'refresh', '--enable', '--root', slash(standards), '--json'], { env: standardsEnv });
fs.appendFileSync(path.join(standards, 'src/item-0.mjs'), '// TODO: reject negative values\n');
fs.cpSync(path.join(standards, '.craft'), standardsSeed, { recursive: true });
const restoreStandards = () => run(process.execPath, [path.join(benchmarks, 'reset-state.mjs'), standardsSeed, path.join(standards, '.craft')]);

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
// Native cells whose peak memory Windows measures separately: Hyperfine records none there.
const memoryCells = [];
const remember = (name, cwd, args, input = null, reset = null, cellEnv = null) => memoryCells.push({ name, program: native, args, cwd, input, reset, env: cellEnv && { CRAFT_EVIDENCE_WARM: cellEnv.CRAFT_EVIDENCE_WARM } });

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
remember('startup/native', root, ['--help']);
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
  remember(`hook/${cell}/native`, root, hookArguments(event).split(' '), payloads[cell], [seed, path.join(root, '.craft')]);
}
// Hooks of a session at work, natively: the cost of each feature as the host reaches it.
const resetStandards = ['--prepare', `node ${quote(path.join(benchmarks, 'reset-state.mjs'))} ${quote(standardsSeed)} ${quote(path.join(standards, '.craft'))}`];
for (const { cell, event, file } of SESSION) {
  measure(`hook/${cell}/native`, standards, ['--warmup', '3', '--runs', String(runs), ...resetStandards, '--input', slash(file), '-N',
    `${quote(native)} ${hookArguments(event)}`], standardsEnv);
  remember(`hook/${cell}/native`, standards, hookArguments(event).split(' '), file, [standardsSeed, path.join(standards, '.craft')], standardsEnv);
}
const refresh = `${quote(native)} project-evidence refresh --enable --root ${quote(project)} --json`;
measure('project-intelligence/refresh-warm/native', project, ['-N', '--warmup', '2', '--runs', String(runs), refresh]);
// cmd.exe /C strips the outer quotes of a prepare line that starts with one: leave the binary unquoted.
measure('project-intelligence/refresh-cold/native', project, ['-N', '--runs', String(coldRuns),
  '--prepare', `${slash(native)} project-evidence rebuild --enable --root ${quote(project)} --json`, refresh]);
remember('project-intelligence/refresh-warm/native', project, ['project-evidence', 'refresh', '--enable', '--root', slash(project), '--json']);
// The background warm-up a session start launches, from an empty index: chunked refreshes with
// pauses for waiting lookups, so its CPU and memory matter more than its wall time.
const index = path.join(project, '.craft/state/intelligence'), empty = path.join(workspace, 'empty');
fs.mkdirSync(empty);
measure('project-intelligence/warm-up/native', project, ['-N', '--runs', String(coldRuns),
  '--prepare', `node ${quote(path.join(benchmarks, 'reset-state.mjs'))} ${quote(empty)} ${quote(index)}`,
  `${quote(native)} project-evidence warm --enable --root ${quote(project)} --json`]);
remember('project-intelligence/warm-up/native', project, ['project-evidence', 'warm', '--enable', '--root', slash(project), '--json'], null, [empty, index]);
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
// Each session hook also delivers its feature: a cell that times a silent hook measures nothing.
for (const { cell, event, file, answers } of SESSION) {
  restoreStandards();
  const result = spawnSync(native, hookArguments(event).split(' '), { cwd: standards, env: standardsEnv, input: fs.readFileSync(file), encoding: 'utf8' });
  let valid = result.stdout === '';
  try { valid ||= JSON.parse(result.stdout) !== null; } catch { valid = false; }
  const answered = answers(result.stdout, standards);
  results.checks.push({ cell, status: result.status, stderr: result.stderr, stdout: result.stdout, answered });
  if (result.status !== 0 || result.stderr || !valid || !answered) results.failures.push(`hook/${cell}: output check failed (exit ${result.status}, answered ${answered})`);
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
// Windows peak working set, per native cell, from the same prepared state the timed runs use.
if (windows) {
  try {
    results.memory = peakMemory({ cells: memoryCells, runs: Math.min(runs, 5), reset: path.join(benchmarks, 'reset-state.mjs'), workspace, env });
    fs.writeFileSync(path.join(output, 'raw/peak-memory.json'), JSON.stringify(results.memory, null, 2) + '\n');
  } catch (error) { results.failures.push(`peak-memory: ${error.message}`); }
}
// Runners of one OS label differ in CPU; history and thresholds compare like with like.
results.testbed = testbed(process.env.TESTBED ?? process.platform, results.platform.cpu);
fs.writeFileSync(path.join(output, 'testbed.txt'), results.testbed + '\n');
save();

const metrics = bencherMetrics(results, file => JSON.parse(fs.readFileSync(path.join(output, file), 'utf8')));
fs.writeFileSync(path.join(output, 'bencher.json'), JSON.stringify(metrics, null, 2) + '\n');
fs.rmSync(workspace, { recursive: true, force: true });
for (const failure of results.failures) console.error(`failed: ${failure}`);
if (results.failures.length) process.exitCode = 1;
