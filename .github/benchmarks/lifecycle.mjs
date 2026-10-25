import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { performance } from 'node:perf_hooks';
import { fileURLToPath, pathToFileURL } from 'node:url';

const self = fileURLToPath(import.meta.url);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const run = (command, args, cwd) => {
  const env = { ...process.env }; delete env.NODE_TEST_CONTEXT;
  const result = spawnSync(command, args, { cwd, env, encoding: 'utf8', timeout: 60000, maxBuffer: 4 * 1024 * 1024 });
  assert.equal(result.status, 0, result.stderr || result.error?.message || result.stdout);
  return result.stdout.trim();
};

if (process.argv[2] === '--worker') {
  const source = process.argv[3];
  const { groundedContext } = await import(pathToFileURL(path.join(source, 'application/session/grounding.mjs')));
  const { stepTask } = await import(pathToFileURL(path.join(source, 'application/session/task.mjs')));
  const { nativeCommand } = await import(pathToFileURL(path.join(source, 'platform/index.mjs')));
  const { sessionStatus } = await import(pathToFileURL(path.join(source, 'application/session/status.mjs')));
  const { readProject } = await import(pathToFileURL(path.join(source, 'application/project/configuration.mjs')));
  const { engineeringContractDigest } = await import(pathToFileURL(path.join(source, 'engine/session/acceptance.mjs')));
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-lifecycle-bench-')));
  try {
    fs.cpSync(path.join(source, 'examples/evals/developer-acceptance/fixture'), root, { recursive: true });
    const git = (...args) => run('git', ['-c', 'core.autocrlf=false', '-c', `core.hooksPath=${path.join(root, '.git', 'no-hooks')}`, '-c', 'commit.gpgsign=false', ...args], root);
    git('init', '-q'); git('add', '.'); git('-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test', 'commit', '-qm', 'Fixture');
    const receipt = path.join(root, 'src/receipt.mjs'), original = fs.readFileSync(receipt, 'utf8');
    const project = readProject(root), samples = [];
    for (const phase of ['cold', 'warm']) {
      fs.writeFileSync(receipt, original);
      const task = `task-${phase}`, spans = {};
      const measure = (name, fn) => { const begin = performance.now(); try { return fn(); } finally { spans[name] = performance.now() - begin; } };
      const started = performance.now(), cpu = process.cpuUsage();
      const context = measure('compose', () => groundedContext(root, { scope: ['src'], sources: [{ id: 'policy', kind: 'instruction', reference: 'AGENTS.md', identity: hash(fs.readFileSync(path.join(root, 'AGENTS.md'))), observedAt: '2026-10-22T00:00:00Z', scope: ['src'], provenance: 'observed', status: 'accepted' }] }).context);
      context.checks = [{ id: 'retry', description: 'node --test acceptance.test.mjs', sourceIds: ['policy'], scope: ['src'] }];
      const contract = { formatVersion: 1, intent: 'Fix canonical retries', exclusions: [], context,
        criteria: [{ id: 'retry', description: 'Canonical retries apply once', required: true, scope: ['src'], sourceIds: ['policy'], checkIds: ['retry'], evidenceKind: 'check' }],
        decisions: [], derivation: { reporter: 'benchmark fixture', method: 'observed', observedAt: '2026-10-22T00:00:00Z' } };
      const contractFile = path.join(root, '.git', `${task}-contract.json`);
      fs.writeFileSync(contractFile, JSON.stringify(contract));
      measure('start', () => assert.equal(nativeCommand(['task-start', '--root', root, '--session', 'benchmark', '--task', task, '--goal', contract.intent, '--file', 'src', '--contract', contractFile], { structured: true }).exitCode, 0));
      fs.writeFileSync(receipt, original.replace('      normalizeKey(key);', '      key = normalizeKey(key);'));
      const status = measure('status', () => sessionStatus({}, project, { root, session: 'benchmark', task }));
      measure('checks', () => run(process.execPath, ['--test', 'acceptance.test.mjs'], root));
      const evidence = { formatVersion: 1, records: [{ id: 'retry', criterionIds: ['retry'], scope: ['src'], contract: engineeringContractDigest(contract), binding: status.binding,
        checkId: 'retry', kind: 'check', artifact: 'benchmark:acceptance-test', observedAt: '2026-10-22T00:00:00Z', reporter: 'benchmark fixture', basis: 'Executable retry and invalid-input assertions passed', support: 'verified', execution: 'passed' }] };
      const input = { stage: 'verify', target: 'acceptance:retry', approach: 'executable-check', evidence: 'benchmark:acceptance-test', outcome: 'passed', result: 'Fixture checks passed', nextStep: 'Report acceptance', acceptanceEvidence: evidence, expectedTaskContract: status.taskContract, expectedEvidenceDigest: status.evidenceDigest, complete: true };
      assert.equal(measure('step', () => stepTask(root, 'benchmark', task, input)).complete, true);
      fs.appendFileSync(receipt, '\n// Source changed after verification.\n');
      const stale = measure('staleStatus', () => sessionStatus({}, project, { root, session: 'benchmark', task }));
      assert.equal(stale.task.goalComplete, false);
      assert.equal(stale.acceptance.records[0].freshness, 'stale');
      assert.throws(() => measure('rejectStale', () => stepTask(root, 'benchmark', task, { ...input, expectedTaskContract: stale.taskContract, expectedEvidenceDigest: stale.evidenceDigest })), /every required criterion/);
      const used = process.cpuUsage(cpu);
      samples.push({ phase, wallMs: performance.now() - started, spans, cpuMs: { user: used.user / 1000, system: used.system / 1000 }, processMaxRssKiB: process.resourceUsage().maxRSS, correct: true });
    }
    console.log(JSON.stringify({ samples }));
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
} else {
  const output = process.argv[2], rounds = Number(process.argv[3] ?? 10), source = process.cwd();
  if (!output || fs.existsSync(output) || !Number.isInteger(rounds) || rounds < 1) throw new Error('Usage: node lifecycle.mjs NEW_OUTPUT [positive rounds]');
  const revision = run('git', ['rev-parse', 'HEAD'], source), status = run('git', ['status', '--porcelain'], source);
  const fixture = path.join(source, 'examples/evals/developer-acceptance/fixture');
  const inventory = directory => fs.readdirSync(directory, { withFileTypes: true }).sort((a,b) => a.name.localeCompare(b.name)).flatMap(entry => entry.isDirectory() ? inventory(path.join(directory, entry.name)) : [{ path: path.relative(fixture, path.join(directory, entry.name)).split(path.sep).join('/'), sha256: hash(fs.readFileSync(path.join(directory, entry.name))) }]);
  const report = { formatVersion: 1, revision, dirty: Boolean(status), patchSha256: hash(run('git', ['diff', 'HEAD', '--binary'], source)), harnessSha256: hash(fs.readFileSync(self)), fixture: inventory(fixture),
    environment: { node: process.version, os: os.platform(), release: os.release(), arch: os.arch(), cpu: os.cpus()[0]?.model, logicalCpus: os.cpus().length, runner: process.env.RUNNER_ENVIRONMENT ?? 'local', git: run('git', ['--version'], source) },
    protocol: { rounds, timing: 'Fresh child per round: cold is first lifecycle; warm repeats in that child with another task and restored source. Module imports and fixture setup excluded from sample wall; processWallMs includes both phases, launch, imports and setup. OS caches uncontrolled.', resources: 'cpuMs and processMaxRssKiB cover the benchmark child only; exclude spawned Git/test processes. maxRSS is cumulative process high-water, so warm is not an independent peak. Whole-process-tree CPU and peak RSS unknown.', effects: 'Synthetic local CLI/application path; task-start runs one native craft process. Includes actual fixture tests. No model, worker or native action dispatch. Timing diagnostic only; no CI latency gate before runner-specific calibration.' }, samples: [] };
  const save = () => fs.writeFileSync(output, JSON.stringify(report, null, 2) + '\n');
  save();
  for (let round = 0; round < rounds; round++) {
    const started = performance.now();
    try {
      const result = JSON.parse(run(process.execPath, [self, '--worker', source], source));
      const processWallMs = performance.now() - started;
      report.samples.push(...result.samples.map(sample => ({ round, processWallMs, ...sample })));
    } catch (error) { report.samples.push({ round, correct: false, error: error.message }); save(); throw error; }
    save();
  }
  const stats = values => { const sorted = values.toSorted((a,b) => a-b), n = sorted.length; return { n, median: (sorted[Math.floor((n-1)/2)] + sorted[Math.floor(n/2)]) / 2, p95: sorted[Math.ceil(n*.95)-1], min: sorted[0], max: sorted.at(-1) }; };
  report.summary = Object.fromEntries(['cold', 'warm'].map(phase => [phase, stats(report.samples.filter(sample => sample.phase === phase).map(sample => sample.wallMs))]));
  report.failures = report.samples.filter(sample => !sample.correct).length;
  save(); console.log(JSON.stringify({ summary: report.summary, failures: report.failures }));
}
