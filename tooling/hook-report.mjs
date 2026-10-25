import { runtimeImports } from './export/export.mjs';
// Measure a local deterministic hook path and report a deliberately bounded policy corpus.
// This does not call a model, run the command text, or install a host hook.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { files } from './lib/capabilities.mjs';
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
// Decide through a generated Claude read-only wrapper in a project configured for read-only review.
export const wrapperPolicy = (wrapper, directory, env = process.env) => ({ command }) => {
  const result = spawnSync(process.execPath, [wrapper], { cwd: directory, env, shell: false, encoding: 'utf8', timeout: 10000,
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
  const wrapper = path.join(repo, 'dist/claude/plugins/craft-review/platform-hooks/read-only.mjs');
  if (!fs.existsSync(wrapper)) throw new Error('build exports before measuring hooks');
  const packageRoot = path.dirname(path.dirname(wrapper));
  const pairs = [
    [path.join(repo, 'packages/craft-review/hooks/read-only.mjs'), path.join(packageRoot, 'hooks/read-only.mjs')],
    ...[['hooks.mjs','hook.mjs'],['registry.mjs','registry.mjs']].map(([source,name]) => [path.join(repo, 'hosts', source), path.join(packageRoot, 'platform-runtime', name)]),
    ...['engine', 'application', 'platform', 'integrations', 'cli'].flatMap(folder => files(path.join(repo, folder)).map(source => [source, path.join(packageRoot, 'support', path.relative(repo, source))]))
  ];
  const digest = (file) => createHash('sha256').update(fs.readFileSync(file)).digest('hex');
  const artifactHashes = {};
  for (const [source, exported] of pairs) {
    if (createHash('sha256').update(runtimeImports(fs.readFileSync(source,'utf8'), exported, packageRoot)).digest('hex') !== digest(exported)) throw new Error('hook exports are stale; rebuild before measuring');
    artifactHashes[path.relative(repo, exported).split(path.sep).join('/')] = digest(exported);
  }
  artifactHashes[path.relative(repo, wrapper).split(path.sep).join('/')] = digest(wrapper);
  // The wrapper decides through the native runtime; without it, shell policy fails closed.
  const env = { ...process.env, CRAFT_NATIVE_BINARY: nativeBinary() };
  const directory = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'hook measurement ')));
  try {
    fs.mkdirSync(path.join(directory, '.craft'));
    fs.writeFileSync(path.join(directory, '.craft/project.json'), JSON.stringify({ formatVersion: 1, review: { readOnly: true } }));
    const samples = [];
    for (let index = 0; index < iterations; index++) {
      const deny = index % 2 === 1;
      const started = performance.now();
      const result = spawnSync(process.execPath, [wrapper], {
        cwd: directory, env, shell: false, encoding: 'utf8', timeout: 10000,
        input: JSON.stringify({ cwd: directory, tool_name: 'Bash', tool_input: { command: deny ? 'git push origin feature' : 'git status --short' } })
      });
      const milliseconds = performance.now() - started;
      if (result.status !== (deny ? 2 : 0)) throw new Error(`unexpected hook process status: ${result.error?.code ?? result.status}`);
      samples.push({ decision: deny ? 'deny' : 'allow', milliseconds });
    }
    const sorted = samples.map((sample) => sample.milliseconds).sort((a, b) => a - b);
    return {
      formatVersion: 1, measuredAt: new Date().toISOString(), platform: process.platform, arch: process.arch, node: process.version, artifactHashes,
      policy: policyReport(wrapperPolicy(wrapper, directory, env)),
      latency: { path: 'fresh Node process → generated Claude wrapper → normalize → native runtime (project read → policy) → serialize', iterations, samples,
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
