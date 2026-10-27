// Measure a local deterministic hook path and report a deliberately bounded policy corpus.
// This does not call a model, run the command text, or install a host hook.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { launchers, hookArguments } from './export/launcher.mjs';
import { performance } from 'node:perf_hooks';
import { spawnSync } from 'node:child_process';
import { nativeBinary } from './lib/native-binary.mjs';

export const CASES = [
  { id: 'local-read', command: 'git status --short', expected: 'allow' },
  { id: 'direct-push', command: 'git push origin feature', expected: 'deny' },
  { id: 'projected-read', command: 'gh pr view 1 --json number', expected: 'allow' },
  { id: 'comment-write', command: 'gh pr comment 1 --body review', expected: 'deny' },
  { id: 'api-write', command: 'gh api -X POST repos/example/project/issues/1/comments', expected: 'deny' },
  { id: 'inert-quoted-text', command: 'echo "git push"', expected: 'allow' },
  { id: 'git-global-directory-option', command: 'git -C project push origin feature', expected: 'deny' },
  { id: 'git-global-output-option', command: 'git --no-pager push origin feature', expected: 'deny' },
  { id: 'quoted-inert-separator', command: 'echo "safe; git push"', expected: 'allow' },
  { id: 'second-command', command: 'git status && git push', expected: 'deny' },
  { id: 'git-config-option', command: 'git -c advice.pushUpdateRejected=false push', expected: 'deny' },
  { id: 'indirect-shell-execution', command: 'sh -c "git push"', expected: 'deny' },
  { id: 'command-substitution', command: 'echo "$(git push)"', expected: 'deny' }
];
// The generated Claude read-only hook as registered: its launcher and arguments.
export function readOnlyHook(repo) {
  const packageRoot = path.join(repo, 'dist/claude/plugins/craft-review');
  const config = path.join(packageRoot, 'platform-hooks/hooks.json');
  if (!fs.existsSync(config)) throw new Error('build exports before measuring hooks');
  const command = JSON.parse(fs.readFileSync(config, 'utf8')).hooks.PreToolUse.flatMap(group => group.hooks)
    .map(hook => /^"\$\{CLAUDE_PLUGIN_ROOT\}\/bin\/craft" (hook claude before-tool .* --handler review-read-only .*)$/.exec(hook.command)).find(Boolean);
  if (!command) throw new Error('no registered read-only review hook');
  return { packageRoot, launcher: path.join(packageRoot, 'bin', 'craft'), args: command[1].split(' ') };
}
const spawnHook = ({ launcher, args }, options) => process.platform === 'win32'
  ? spawnSync(`"${launcher}.cmd"`, args, { ...options, shell: true }) : spawnSync(launcher, args, { ...options, shell: false });
// Decide through the generated Claude read-only hook in a project configured for read-only review.
export const hookPolicy = (hook, directory, env = process.env) => ({ command }) => {
  const result = spawnHook(hook, { cwd: directory, env, encoding: 'utf8', timeout: 10000,
    input: JSON.stringify({ cwd: directory, tool_name: 'Bash', tool_input: { command } }) });
  return { decision: result.status === 2 ? 'deny' : result.status === 0 ? 'allow' : undefined };
};
export function policyReport(policy, cases = CASES) {
  const results = cases.map((entry) => {
    const observed = policy({ command: entry.command }).decision;
    return { ...entry, observed };
  });
  return {
    cases: results,
    falseBlocks: results.filter((entry) => entry.expected === 'allow' && entry.observed === 'deny').length,
    missedBlocks: results.filter((entry) => entry.expected === 'deny' && entry.observed === 'allow').length,
    unexpectedDecisions: results.filter((entry) => !['allow', 'deny'].includes(entry.observed)).length,
    limitation: 'Hand-labeled diagnostic examples, not a sampled production distribution or complete shell grammar.'
  };
}
export function localHookReport(repo, iterations = 12) {
  if (!Number.isInteger(iterations) || iterations < 1 || iterations > 100) throw new Error('iterations must be an integer from 1 to 100');
  const hook = readOnlyHook(repo);
  const source = path.join(repo, 'packages/craft-review');
  const item = { root: source, manifest: JSON.parse(fs.readFileSync(path.join(source, 'capability.json'), 'utf8')) };
  const definition = item.manifest.hooks.find(entry => entry.name === 'read-only');
  const digest = (bytes) => createHash('sha256').update(bytes).digest('hex');
  // The hook path is the generated launcher and the registered arguments.
  const expected = launchers(item, 'claude').map(([name, text]) => [text, path.join(hook.packageRoot, name)]);
  const artifactHashes = {};
  for (const [text, exported] of expected) {
    if (!fs.existsSync(exported) || digest(text) !== digest(fs.readFileSync(exported, 'utf8'))) throw new Error('hook exports are stale; rebuild before measuring');
    artifactHashes[path.relative(repo, exported).split(path.sep).join('/')] = digest(fs.readFileSync(exported));
  }
  if (hookArguments('claude', definition, item).join(' ') !== hook.args.join(' ')) throw new Error('hook exports are stale; rebuild before measuring');
  // The launcher runs the native runtime under test; without one, shell policy fails closed.
  const env = { ...process.env, CRAFT_NATIVE_BINARY: nativeBinary() };
  const directory = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'hook measurement ')));
  try {
    fs.mkdirSync(path.join(directory, '.craft'));
    fs.writeFileSync(path.join(directory, '.craft/project.json'), JSON.stringify({ formatVersion: 1, review: { readOnly: true } }));
    const samples = [];
    for (let index = 0; index < iterations; index++) {
      const deny = index % 2 === 1;
      const started = performance.now();
      const result = spawnHook(hook, {
        cwd: directory, env, encoding: 'utf8', timeout: 10000,
        input: JSON.stringify({ cwd: directory, tool_name: 'Bash', tool_input: { command: deny ? 'git push origin feature' : 'git status --short' } })
      });
      const milliseconds = performance.now() - started;
      if (result.status !== (deny ? 2 : 0)) throw new Error(`unexpected hook process status: ${result.error?.code ?? result.status}`);
      samples.push({ decision: deny ? 'deny' : 'allow', milliseconds });
    }
    const sorted = samples.map((sample) => sample.milliseconds).sort((a, b) => a - b);
    return {
      formatVersion: 1, measuredAt: new Date().toISOString(), platform: process.platform, arch: process.arch, node: process.version, artifactHashes,
      policy: policyReport(hookPolicy(hook, directory, env)),
      latency: { path: 'generated Claude launcher → native runtime (payload → project read → policy → wire output)', iterations, samples,
        minMs: sorted[0], medianMs: sorted[Math.floor(sorted.length / 2)], maxMs: sorted.at(-1),
        limitation: 'Local process-wall-time samples only; no host dispatch, model call, checkpoint write or network. Not a cross-platform or production latency claim.' }
    };
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
}
if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  try {
    const args = process.argv.slice(2);
    const options = {};
    for (let index = 0; index < args.length; index += 2) {
      const key = args[index]; const value = args[index + 1];
      if (!['--iterations', '--output'].includes(key) || options[key] !== undefined || !value || value.startsWith('--')) throw new Error('usage: node tooling/hook-report.mjs [--iterations 1..100] [--output file]');
      options[key] = value;
    }
    const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
    const report = localHookReport(repo, options['--iterations'] === undefined ? 12 : Number(options['--iterations']));
    const text = `${JSON.stringify(report, null, 2)}\n`;
    if (options['--output']) {
      const output = path.resolve(options['--output']);
      fs.mkdirSync(path.dirname(output), { recursive: true });
      fs.writeFileSync(output, text, { flag: 'wx' });
      process.stdout.write(`Hook diagnostic report saved: ${output}\n`);
    } else process.stdout.write(text);
  } catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 2; }
}
