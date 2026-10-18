#!/usr/bin/env node
// Provider-independent, explicitly invoked collection of one frozen baseline/candidate pair.
// No automatic model choice, semantic judging, retries, credential setup or publication.
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { gitState } from '../application/checkpoint.mjs';

const sha = (content) => createHash('sha256').update(content).digest('hex');
const writeJson = (file, data) => fs.writeFileSync(file, `${JSON.stringify(data, null, 2)}\n`, { flag: 'wx' });
function fileBytes(file) {
  const stat = fs.lstatSync(file);
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size > 1024 * 1024) throw new Error(`expected bounded regular file: ${file}`);
  return fs.readFileSync(file);
}
function inventory(directory) {
  const files = []; let bytes = 0;
  function visit(root) {
    if (fs.lstatSync(root).isSymbolicLink()) throw new Error('eval fixtures must not traverse symlinks');
    for (const entry of fs.readdirSync(root, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      const full = path.join(root, entry.name);
      if (entry.isSymbolicLink()) throw new Error('eval fixtures must not traverse symlinks');
      if (entry.isDirectory()) visit(full);
      else {
        const content = fileBytes(full); bytes += content.length;
        files.push({ path: path.relative(directory, full).split(path.sep).join('/'), sha256: sha(content), bytes: content.length });
        if (files.length > 1000 || bytes > 16 * 1024 * 1024) throw new Error('eval fixture exceeds inventory limits');
      }
    }
  }
  visit(directory);
  return files;
}
function validateRunner(runner) {
  if (!runner || typeof runner.command !== 'string' || !runner.command.trim() || !Array.isArray(runner.args)
    || runner.args.some((arg) => typeof arg !== 'string') || typeof runner.identity !== 'string' || !runner.identity.trim()
    || !Number.isSafeInteger(runner.timeoutMs) || runner.timeoutMs < 1 || runner.timeoutMs > 300000) throw new Error('runner requires command, argv, identity and timeoutMs (1..300000)');
}
export function preparePair(configFile, output, sourceRoot) {
  output = path.resolve(output);
  const config = JSON.parse(fileBytes(configFile).toString('utf8'));
  validateRunner(config.runner);
  const order = config.order ?? ['baseline', 'candidate'];
  if (!Array.isArray(order) || order.length !== 2 || new Set(order).size !== 2 || !order.includes('baseline') || !order.includes('candidate')) throw new Error('order must contain baseline and candidate exactly once');
  if (!Array.isArray(config.guidance) || !config.guidance.length || config.guidance.length > 30 || config.guidance.some((file) => typeof file !== 'string')) throw new Error('explicit guidance files required');
  if (typeof config.fixture !== 'string' || typeof config.prompt !== 'string') throw new Error('fixture and prompt paths required');
  const relative = (file) => path.resolve(path.dirname(configFile), file);
  const fixture = relative(config.fixture);
  const frozen = inventory(fixture);
  const prompt = fileBytes(relative(config.prompt)).toString('utf8');
  if (!prompt.trim()) throw new Error('eval prompt is empty');
  const guidance = config.guidance.map((file) => ({ source: file, content: fileBytes(relative(file)).toString('utf8') }));
  if (guidance.reduce((sum, entry) => sum + Buffer.byteLength(entry.content), 0) > 1024 * 1024) throw new Error('guidance exceeds size limit');
  // Refuse pre-existing output; neither preparation nor execution overwrites prior evidence.
  fs.mkdirSync(output, { recursive: false });
  fs.mkdirSync(path.join(output, 'guidance'));
  guidance.forEach((entry, index) => fs.writeFileSync(path.join(output, 'guidance', `${index}.md`), entry.content, { flag: 'wx' }));
  for (const arm of ['baseline', 'candidate']) {
    const directory = path.join(output, arm);
    fs.mkdirSync(directory);
    fs.cpSync(fixture, path.join(directory, 'workspace'), { recursive: true });
    const extra = arm === 'candidate' ? `\n\nSelected capability guidance:\n${guidance.map((entry) => entry.content).join('\n\n')}` : '';
    fs.writeFileSync(path.join(directory, 'prompt.txt'), `${prompt}${extra}`, { flag: 'wx' });
  }
  const record = { formatVersion: 1, preparedAt: new Date().toISOString(), runner: config.runner, order,
    sourceState: gitState(sourceRoot), fixture: frozen, promptSha256: sha(prompt),
    guidance: guidance.map((entry, index) => ({ source: entry.source, artifact: `guidance/${index}.md`, sha256: sha(entry.content), bytes: Buffer.byteLength(entry.content) })),
    arms: Object.fromEntries(['baseline', 'candidate'].map((arm) => [arm, { promptSha256: sha(fileBytes(path.join(output, arm, 'prompt.txt'))) }])),
    limits: 'One pair, explicit recorded arm order. Runner identity is declared; inspect actual runner metadata. Raw output and failures are evidence, not a semantic verdict. No cost or token usage is inferred.' };
  writeJson(path.join(output, 'pair.json'), record);
  return record;
}
export function runPair(output) {
  output = path.resolve(output);
  if (fs.lstatSync(output).isSymbolicLink()) throw new Error('eval output must not be a symlink');
  const record = JSON.parse(fileBytes(path.join(output, 'pair.json')).toString('utf8'));
  validateRunner(record.runner);
  fs.writeFileSync(path.join(output, 'run.lock'), `${process.pid}\n`, { flag: 'wx' });
  const validateArm = (arm) => {
    const directory = path.join(output, arm);
    if (fs.existsSync(path.join(directory, 'result.json')) || fs.existsSync(path.join(directory, 'stdout.txt')) || fs.existsSync(path.join(directory, 'stderr.txt'))) throw new Error('pair already attempted; prepare a fresh output directory');
    const prompt = fileBytes(path.join(directory, 'prompt.txt'));
    if (sha(prompt) !== record.arms[arm].promptSha256
      || JSON.stringify(inventory(path.join(directory, 'workspace'))) !== JSON.stringify(record.fixture)) throw new Error('prepared eval input changed');
    return prompt;
  };
  // Preflight both arms before spending model calls; existing/modified evidence is preserved.
  for (const arm of record.order ?? ['baseline', 'candidate']) validateArm(arm);
  const results = {};
  for (const arm of record.order ?? ['baseline', 'candidate']) {
    const directory = path.join(output, arm), workspace = path.join(directory, 'workspace');
    // A preceding runner may have changed its sibling. Recheck at the invocation boundary
    // and pass the validated prompt bytes, rather than reopening the prompt afterward.
    const prompt = validateArm(arm);
    const started = Date.now();
    const result = spawnSync(record.runner.command, record.runner.args.map((arg) => arg.replaceAll('{workspace}', workspace)), {
      cwd: workspace, input: prompt, encoding: 'utf8',
      timeout: record.runner.timeoutMs, maxBuffer: 2 * 1024 * 1024, shell: false
    });
    fs.writeFileSync(path.join(directory, 'stdout.txt'), result.stdout ?? '', { flag: 'wx' });
    fs.writeFileSync(path.join(directory, 'stderr.txt'), result.stderr ?? '', { flag: 'wx' });
    let after, inspectionError = null;
    try { after = inventory(workspace); } catch (error) { inspectionError = error.message; }
    const entry = { exitCode: result.status, signal: result.signal, error: result.error?.message ?? null,
      durationMs: Date.now() - started, responsePresent: Boolean(result.stdout?.trim()),
      workspaceAfter: after ?? null, inspectionError,
      tokenUsage: null, cost: null, semanticVerdict: 'not-judged' };
    writeJson(path.join(directory, 'result.json'), entry);
    results[arm] = entry;
    // An unavailable runner is a setup failure; do not repeat the same failing invocation.
    if (result.error || result.status !== 0 || !entry.responsePresent) break;
  }
  const summary = { formatVersion: 1, collectionComplete: Object.keys(results).length === 2
    && Object.values(results).every((entry) => entry.exitCode === 0 && entry.responsePresent && !entry.error && !entry.inspectionError),
  results, semanticVerdict: 'not-judged; acceptance and model identity require review of retained evidence' };
  writeJson(path.join(output, 'collection.json'), summary);
  return summary;
}
if (process.argv[1] && fileURLToPath(import.meta.url) === fs.realpathSync(process.argv[1])) {
  try {
    const [action, ...args] = process.argv.slice(2);
    let result;
    if (action === 'prepare' && args.length === 2) result = preparePair(path.resolve(args[0]), path.resolve(args[1]), fileURLToPath(new URL('../', import.meta.url)));
    else if (action === 'run' && args.length === 1) result = runPair(path.resolve(args[0]));
    else throw new Error('usage: paired-eval.mjs prepare <config.json> <new-output-directory> | run <prepared-output-directory>');
    process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
    if (result.collectionComplete === false) process.exitCode = 1;
  } catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 2; }
}
