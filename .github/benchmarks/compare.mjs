// Compare two installed Claude exports the way a host runs them: every declared hook
// command through bash with a host payload, and CLI calls as an agent's shell issues them.
// Rounds alternate variants; each variant owns a disposable consumer root and home.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {performance} from 'node:perf_hooks';

const [baseline, candidate, output, roundsText = '20'] = process.argv.slice(2);
const rounds = Number(roundsText);
if (!baseline || !candidate || !output || process.argv.length > 6 || !Number.isInteger(rounds) || rounds < 2)
  throw Error('Usage: node compare.mjs BASELINE_CLAUDE_EXPORT CANDIDATE_CLAUDE_EXPORT RESULT_JSON [ROUNDS>=2]');
const windows = process.platform === 'win32';
const temporary = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-compare-')));
const plugins = ['craft-delivery', 'craft-engineering'];

const git = (cwd, ...args) => {
  const r = spawnSync('git', args, {cwd, encoding: 'utf8'});
  if (r.status !== 0) throw Error(`git ${args.join(' ')}: ${r.stderr}`);
};
const consumer = name => {
  const root = path.join(temporary, name);
  fs.mkdirSync(path.join(root, 'src'), {recursive: true});
  for (let n = 0; n < 200; n++) fs.writeFileSync(path.join(root, 'src', `item-${n}.mjs`), `export function item${n}(value) {\n  return value + ${n};\n}\n`);
  fs.mkdirSync(path.join(root, '.craft'));
  fs.writeFileSync(path.join(root, '.craft/project.json'), JSON.stringify({formatVersion: 1}) + '\n');
  git(root, 'init', '--quiet');
  git(root, '-c', 'user.name=bench', '-c', 'user.email=bench@example.invalid', 'add', '.');
  git(root, '-c', 'user.name=bench', '-c', 'user.email=bench@example.invalid', 'commit', '--quiet', '-m', 'fixture');
  const home = path.join(temporary, `${name}-home`);
  fs.mkdirSync(home);
  return {root, home};
};

const hooksOf = exportRoot => Object.fromEntries(plugins.map(plugin => {
  const directory = path.join(exportRoot, 'plugins', plugin);
  const file = ['platform-hooks/hooks.json', 'hooks/hooks.json'].map(f => path.join(directory, f)).find(f => fs.existsSync(f));
  if (!file) throw Error(`no hooks.json for ${plugin} in ${exportRoot}`);
  return [plugin, {directory, hooks: JSON.parse(fs.readFileSync(file, 'utf8')).hooks}];
}));
// Commands a host would run for one event, in declaration order across both plugins.
const commandsFor = (declared, event, matcherValue) => Object.entries(declared).flatMap(([plugin, {directory, hooks}]) =>
  (hooks[event] ?? []).filter(group => !group.matcher || matcherValue === undefined || new RegExp(`^(?:${group.matcher})$`).test(matcherValue))
    .flatMap(group => group.hooks.filter(h => h.type === 'command').map(h => ({plugin, directory, command: h.command}))));

const toolUse = (tool, input) => ({tool_name: tool, tool_input: input, tool_use_id: `toolu_${tool}`});
const scenario = root => {
  const file = path.join(root, 'src', 'item-0.mjs');
  return [
    {event: 'SessionStart', matcher: 'startup', payload: {source: 'startup'}},
    {event: 'UserPromptSubmit', payload: {prompt: 'Implement a guard in src/item-0.mjs and add a test'}},
    {event: 'PreToolUse', label: 'PreToolUse:Read', matcher: 'Read', payload: toolUse('Read', {file_path: file})},
    {event: 'PostToolUse', label: 'PostToolUse:Read', matcher: 'Read', payload: {...toolUse('Read', {file_path: file}), tool_response: {type: 'text'}}},
    {event: 'PreToolUse', label: 'PreToolUse:Write', matcher: 'Write', payload: toolUse('Write', {file_path: file, content: 'x'})},
    {event: 'PostToolUse', label: 'PostToolUse:Write', matcher: 'Write', payload: {...toolUse('Write', {file_path: file, content: 'x'}), tool_response: {success: true}}},
    {event: 'PreToolUse', label: 'PreToolUse:Bash', matcher: 'Bash', payload: toolUse('Bash', {command: 'git status --short'})},
    {event: 'PostToolUse', label: 'PostToolUse:Bash', matcher: 'Bash', payload: {...toolUse('Bash', {command: 'git status --short'}), tool_response: {stdout: '', exit_code: 0}}},
    {event: 'Stop', payload: {stop_hook_active: false}},
    {event: 'PreCompact', matcher: 'auto', payload: {trigger: 'auto'}},
    {event: 'SessionStart', label: 'SessionStart:compact', matcher: 'compact', payload: {source: 'compact'}},
  ];
};
const cliCalls = [
  ['route', 'route --intent implement --file src/item-0.mjs'],
  ['checkpoint-save', 'checkpoint-save --session bench-session'],
  ['checkpoint-read', 'checkpoint-read --session bench-session'],
  ['session', 'session --session bench-session'],
  ['context-report', 'context-report --intent implement'],
  ['locate', 'locate --term item0'],
];
// Hosts run hooks through Git Bash on Windows; `bash` on PATH there may be WSL instead.
const gitBash = 'C:\\Program Files\\Git\\bin\\bash.exe';
const bash = process.env.BENCHMARK_BASH ?? (windows && fs.existsSync(gitBash) ? gitBash : 'bash');
const quote = value => `'${value.replaceAll("'", `'\\''`)}'`;
const shellPath = value => (windows ? value.replaceAll('\\', '/') : value);

const variants = [['baseline', path.resolve(baseline)], ['candidate', path.resolve(candidate)]].map(([name, exportRoot]) => {
  const declared = hooksOf(exportRoot);
  const {root, home} = consumer(name);
  const delivery = shellPath(declared['craft-delivery'].directory);
  const legacyCli = path.join(declared['craft-delivery'].directory, 'support/cli/main.mjs');
  const cli = fs.existsSync(path.join(declared['craft-delivery'].directory, 'bin/craft'))
    ? `${quote(`${delivery}/bin/craft`)}` : `node ${quote(shellPath(legacyCli))}`;
  return {name, exportRoot, declared, root, home, cli};
});

const isolatedEnv = (variant, extra = {}) => {
  const env = {...process.env, HOME: variant.home, USERPROFILE: variant.home, XDG_CONFIG_HOME: path.join(variant.home, '.config'),
    XDG_STATE_HOME: path.join(variant.home, '.local/state'), XDG_DATA_HOME: path.join(variant.home, '.local/share'), ...extra};
  for (const key of Object.keys(env)) if (/^(CRAFT_|OTEL_)/.test(key)) delete env[key];
  return env;
};
const timeTool = process.platform === 'darwin' ? ['/usr/bin/time', ['-l']] : process.platform === 'linux' && fs.existsSync('/usr/bin/time') ? ['/usr/bin/time', ['-v']] : null;
const peakRss = stderr => {
  const mac = stderr.match(/(\d+)\s+maximum resident set size/);
  if (mac) return Number(mac[1]);
  const gnu = stderr.match(/Maximum resident set size \(kbytes\): (\d+)/);
  return gnu ? Number(gnu[1]) * 1024 : null;
};
const execute = (variant, command, {input, pluginRoot, measureRss = false} = {}) => {
  const env = isolatedEnv(variant, pluginRoot ? {CLAUDE_PLUGIN_ROOT: shellPath(pluginRoot), CLAUDE_PROJECT_DIR: variant.root} : {});
  const [exe, args] = measureRss && timeTool ? [timeTool[0], [...timeTool[1], bash, '-c', command]] : [bash, ['-c', command]];
  const start = performance.now();
  const r = spawnSync(exe, args, {cwd: variant.root, env, input, encoding: 'utf8', timeout: 60000, maxBuffer: 16 * 1024 * 1024});
  const ms = performance.now() - start;
  if (r.error || r.status !== 0) throw Error(`${variant.name}: ${command} exited ${r.status ?? r.error?.message}: ${r.stderr.slice(-800)}`);
  if (/native runtime unavailable/.test(r.stdout + r.stderr)) throw Error(`${variant.name}: ${command} fell back: ${r.stdout}`);
  return {ms, rss: measureRss ? peakRss(r.stderr) : undefined};
};

// Each step is a list of host commands; a host runs one event's hooks concurrently, so
// both the sum (CPU/process cost) and the slowest command (latency floor) are reported.
const steps = variant => [
  ...scenario(variant.root).map(step => ({
    name: `hook ${step.label ?? step.event}`,
    commands: commandsFor(variant.declared, step.event, step.matcher).map(c => ({
      plugin: c.plugin, pluginRoot: c.directory,
      command: c.command.replaceAll('${CLAUDE_PLUGIN_ROOT}', shellPath(c.directory)),
      input: JSON.stringify({session_id: 'bench-session', transcript_path: path.join(variant.home, 'transcript.jsonl'), cwd: variant.root, hook_event_name: step.event, ...step.payload}),
    })),
  })),
  ...cliCalls.map(([name, args]) => ({name: `cli ${name}`, commands: [{command: `${variant.cli} ${args} --root ${quote(shellPath(variant.root))} --json`}]})),
];

const samples = new Map();
const record = (variant, step, index, value) => {
  const key = `${variant.name}\0${step}`;
  if (!samples.has(key)) samples.set(key, {commands: [], sum: [], max: []});
  const entry = samples.get(key);
  (entry.commands[index] ??= []).push(value);
};
for (let round = 0; round < rounds; round++) {
  const order = round % 2 ? [...variants].reverse() : variants;
  for (const variant of order) for (const step of steps(variant)) {
    const times = step.commands.map((c, i) => {
      const {ms} = execute(variant, c.command, c);
      record(variant, step.name, i, ms);
      return ms;
    });
    const entry = samples.get(`${variant.name}\0${step.name}`) ?? {commands: [], sum: [], max: []};
    samples.set(`${variant.name}\0${step.name}`, entry);
    entry.sum.push(times.reduce((a, b) => a + b, 0));
    entry.max.push(Math.max(0, ...times));
  }
}
const rss = {};
for (const variant of variants) for (const step of steps(variant)) {
  const values = step.commands.map(c => execute(variant, c.command, {...c, measureRss: true}).rss);
  rss[`${variant.name}\0${step.name}`] = values.every(v => typeof v === 'number') ? Math.max(...values) : null;
}

const quantile = (values, q) => {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))];
};
const describe = values => ({first: values[0], median: quantile(values.slice(1), 0.5), p90: quantile(values.slice(1), 0.9), samples: values.length});
const directoryBytes = directory => fs.readdirSync(directory, {withFileTypes: true}).reduce((total, entry) => {
  const file = path.join(directory, entry.name);
  return total + (entry.isDirectory() ? directoryBytes(file) : entry.isFile() ? fs.statSync(file).size : 0);
}, 0);
const report = {
  formatVersion: 1, measuredAt: new Date().toISOString(), rounds,
  environment: {platform: process.platform, release: os.release(), arch: os.arch(), node: process.version, cpu: os.cpus()[0]?.model, logicalCpus: os.cpus().length,
    rssTool: timeTool ? timeTool[0] : null, runner: Object.fromEntries(['RUNNER_OS', 'ImageVersion', 'GITHUB_RUN_ID', 'GITHUB_SHA'].filter(k => process.env[k]).map(k => [k, process.env[k]]))},
  method: 'Each command runs through bash -c as a host does; first is the first run in a new consumer root, median/p90 exclude it. Peak RSS is the largest process in the command tree from one extra run.',
  variants: Object.fromEntries(variants.map(v => [v.name, {export: v.exportRoot, cli: v.cli, installedBytes: Object.fromEntries(plugins.map(p => [p, directoryBytes(path.join(v.exportRoot, 'plugins', p))]))}])),
  steps: [...new Set(steps(variants[0]).map(s => s.name))].map(name => ({
    name,
    ...Object.fromEntries(variants.map(v => {
      const entry = samples.get(`${v.name}\0${name}`);
      return [v.name, {commands: entry.commands.length, sumMs: describe(entry.sum), slowestMs: describe(entry.max), peakRssBytes: rss[`${v.name}\0${name}`]}];
    })),
  })),
};
fs.writeFileSync(output, JSON.stringify(report, null, 2) + '\n');

const fmt = value => (value === null || value === undefined ? 'n/a' : value.toFixed(1));
const mib = value => (value ? (value / 1048576).toFixed(1) : 'n/a');
const lines = [`Baseline vs candidate on ${report.environment.platform}/${report.environment.arch}, ${rounds} rounds (ms, median of warm runs; RSS MiB)`, '',
  '| Step | Cmds | Baseline sum | Candidate sum | Δ | Baseline slowest | Candidate slowest | Baseline RSS | Candidate RSS |', '|---|---|---|---|---|---|---|---|---|'];
let totals = [0, 0];
for (const step of report.steps) {
  const [b, c] = [step.baseline, step.candidate];
  totals = [totals[0] + b.sumMs.median, totals[1] + c.sumMs.median];
  lines.push(`| ${step.name} | ${b.commands}/${c.commands} | ${fmt(b.sumMs.median)} | ${fmt(c.sumMs.median)} | ${(c.sumMs.median / b.sumMs.median).toFixed(2)}× | ${fmt(b.slowestMs.median)} | ${fmt(c.slowestMs.median)} | ${mib(b.peakRssBytes)} | ${mib(c.peakRssBytes)} |`);
}
lines.push(`| **total** | | ${fmt(totals[0])} | ${fmt(totals[1])} | ${(totals[1] / totals[0]).toFixed(2)}× | | | | |`, '',
  `Installed bytes: ${variants.map(v => `${v.name} ${plugins.map(p => `${p} ${mib(report.variants[v.name].installedBytes[p])} MiB`).join(', ')}`).join('; ')}`, '');
const summary = lines.join('\n');
fs.writeFileSync(output.replace(/\.json$/, '') + '.md', summary);
if (process.env.GITHUB_STEP_SUMMARY) fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, summary);
console.log(summary);
fs.rmSync(temporary, {recursive: true, force: true});
