#!/usr/bin/env node
// Provider-independent, explicitly invoked collection of one frozen baseline/candidate pair.
// No automatic model choice, semantic judging, retries, credential setup or publication.
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { repositorySnapshot as gitState } from '../application/repositories.mjs';

const sha = (content) => createHash('sha256').update(content).digest('hex');
const writeJson = (file, data) => fs.writeFileSync(file, `${JSON.stringify(data, null, 2)}\n`, { flag: 'wx' });
function fileBytes(file) {
  const stat = fs.lstatSync(file);
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size > 1024 * 1024) throw new Error(`expected bounded regular file: ${file}`);
  return fs.readFileSync(file);
}
function inventory(directory, { maxFiles = 1000, maxBytes = 16 * 1024 * 1024 } = {}) {
  const files = []; let bytes = 0, directories = 0;
  function visit(root, depth = 0) {
    if (++directories > maxFiles || depth > 64) throw new Error('eval directory exceeds traversal limits');
    if (fs.lstatSync(root).isSymbolicLink()) throw new Error('eval fixtures must not traverse symlinks');
    for (const entry of fs.readdirSync(root, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      const full = path.join(root, entry.name);
      if (entry.isSymbolicLink()) throw new Error('eval fixtures must not traverse symlinks');
      if (entry.isDirectory()) visit(full, depth + 1);
      else {
        const content = fileBytes(full); bytes += content.length;
        files.push({ path: path.relative(directory, full).split(path.sep).join('/'), sha256: sha(content), bytes: content.length });
        if (files.length > maxFiles || bytes > maxBytes) throw new Error('eval fixture exceeds inventory limits');
      }
    }
  }
  visit(directory);
  return files;
}
const bundleLimits = { maxFiles: 10000, maxBytes: 64 * 1024 * 1024 };
function profileFiles(files, relative) {
  if (files === undefined) return [];
  if (!files || typeof files !== 'object' || Array.isArray(files) || Object.keys(files).length > 32) throw new Error('profileFiles must be a bounded relative-path/source map');
  return Object.entries(files).map(([name, source]) => {
    if (!name || path.isAbsolute(name) || name.includes('\\') || name.split('/').some(part => !part || part === '.' || part === '..') || typeof source !== 'string') throw new Error('invalid isolated profile path');
    return { path: name, content: fileBytes(relative(source)) };
  });
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
  const integrated = config.bundles !== undefined;
  if (integrated) {
    if (!config.bundles || typeof config.bundles !== 'object' || Array.isArray(config.bundles)
      || Object.keys(config.bundles).length !== 2 || !['baseline', 'candidate'].every(arm => typeof config.bundles[arm] === 'string')
      || config.guidance !== undefined) throw new Error('isolated bundle mode requires baseline/candidate bundles and no prompt guidance injection');
    if (!['{bundle}', '{profile}'].every(placeholder => config.runner.args.some(arg => arg.includes(placeholder)))) throw new Error('isolated bundle runner must explicitly consume {bundle} and {profile} paths');
    if (config.profileFiles !== undefined && (!config.profileFiles || typeof config.profileFiles !== 'object' || Array.isArray(config.profileFiles)
      || Object.keys(config.profileFiles).some(arm => !['baseline', 'candidate'].includes(arm)))) throw new Error('invalid per-arm profileFiles');
  } else if (!Array.isArray(config.guidance) || !config.guidance.length || config.guidance.length > 30 || config.guidance.some((file) => typeof file !== 'string')) throw new Error('explicit guidance files required');
  if (typeof config.fixture !== 'string' || typeof config.prompt !== 'string') throw new Error('fixture and prompt paths required');
  const relative = (file) => path.resolve(path.dirname(configFile), file);
  const fixture = relative(config.fixture);
  const frozen = inventory(fixture);
  const prompt = fileBytes(relative(config.prompt)).toString('utf8');
  if (!prompt.trim()) throw new Error('eval prompt is empty');
  const guidance = (config.guidance ?? []).map((file) => ({ source: file, content: fileBytes(relative(file)).toString('utf8') }));
  if (guidance.reduce((sum, entry) => sum + Buffer.byteLength(entry.content), 0) > 1024 * 1024) throw new Error('guidance exceeds size limit');
  const bundles = integrated ? Object.fromEntries(['baseline', 'candidate'].map(arm => {
    const directory = relative(config.bundles[arm]), files = inventory(directory, bundleLimits);
    if (!files.length) throw new Error('isolated runtime bundle must not be empty');
    const profile = profileFiles(config.profileFiles?.[arm], relative);
    if (profile.reduce((sum, entry) => sum + entry.content.length, 0) > 1024 * 1024) throw new Error('isolated profile exceeds size limit');
    return [arm, { directory, files, profile }];
  })) : null;
  // Refuse pre-existing output; neither preparation nor execution overwrites prior evidence.
  fs.mkdirSync(output, { recursive: false });
  fs.mkdirSync(path.join(output, 'guidance'));
  guidance.forEach((entry, index) => fs.writeFileSync(path.join(output, 'guidance', `${index}.md`), entry.content, { flag: 'wx' }));
  for (const arm of ['baseline', 'candidate']) {
    const directory = path.join(output, arm);
    fs.mkdirSync(directory);
    fs.cpSync(fixture, path.join(directory, 'workspace'), { recursive: true });
    if (integrated) {
      fs.cpSync(bundles[arm].directory, path.join(directory, 'bundle'), { recursive: true });
      fs.mkdirSync(path.join(directory, 'profile'));
      for (const entry of bundles[arm].profile) {
        const target = path.join(directory, 'profile', entry.path);
        fs.mkdirSync(path.dirname(target), { recursive: true }); fs.writeFileSync(target, entry.content, { flag: 'wx', mode: 0o600 });
      }
      if (JSON.stringify(inventory(path.join(directory, 'bundle'), bundleLimits)) !== JSON.stringify(bundles[arm].files)) throw new Error('bundle changed while freezing the pair');
    }
    const extra = !integrated && arm === 'candidate' ? `\n\nSelected capability guidance:\n${guidance.map((entry) => entry.content).join('\n\n')}` : '';
    fs.writeFileSync(path.join(directory, 'prompt.txt'), `${prompt}${extra}`, { flag: 'wx' });
  }
  const record = { formatVersion: integrated ? 2 : 1, mode: integrated ? 'isolated-bundles' : 'prompt-guidance', preparedAt: new Date().toISOString(), runner: config.runner, order,
    sourceState: gitState(sourceRoot), fixture: frozen, promptSha256: sha(prompt),
    guidance: guidance.map((entry, index) => ({ source: entry.source, artifact: `guidance/${index}.md`, sha256: sha(entry.content), bytes: Buffer.byteLength(entry.content) })),
    arms: Object.fromEntries(['baseline', 'candidate'].map((arm) => [arm, { promptSha256: sha(fileBytes(path.join(output, arm, 'prompt.txt'))),
      ...(integrated ? { bundle: bundles[arm].files, profile: inventory(path.join(output, arm, 'profile')),
        bundleIdentity: sha(JSON.stringify(bundles[arm].files)) } : {}) }])),
    limits: 'One pair, explicit recorded arm order. Runner identity is declared; inspect actual runner metadata. Raw output and failures are evidence, not a semantic verdict. No cost or token usage is inferred.' };
  writeJson(path.join(output, 'pair.json'), record);
  return record;
}
export function runPair(output) {
  output = path.resolve(output);
  if (fs.lstatSync(output).isSymbolicLink()) throw new Error('eval output must not be a symlink');
  const record = JSON.parse(fileBytes(path.join(output, 'pair.json')).toString('utf8'));
  validateRunner(record.runner);
  if (!((record.formatVersion === 1 && (record.mode === undefined || record.mode === 'prompt-guidance'))
    || (record.formatVersion === 2 && record.mode === 'isolated-bundles'))
    || !Array.isArray(record.order ?? ['baseline', 'candidate'])
    || (record.order ?? ['baseline', 'candidate']).length !== 2
    || !['baseline', 'candidate'].every(arm => (record.order ?? ['baseline', 'candidate']).includes(arm))) throw new Error('invalid prepared pair mode/order');
  fs.writeFileSync(path.join(output, 'run.lock'), `${process.pid}\n`, { flag: 'wx' });
  const validateArm = (arm) => {
    const directory = path.join(output, arm);
    if (fs.existsSync(path.join(directory, 'result.json')) || fs.existsSync(path.join(directory, 'stdout.txt')) || fs.existsSync(path.join(directory, 'stderr.txt'))) throw new Error('pair already attempted; prepare a fresh output directory');
    const prompt = fileBytes(path.join(directory, 'prompt.txt'));
    if (sha(prompt) !== record.arms[arm].promptSha256
      || JSON.stringify(inventory(path.join(directory, 'workspace'))) !== JSON.stringify(record.fixture)) throw new Error('prepared eval input changed');
    if (record.mode === 'isolated-bundles' && (JSON.stringify(inventory(path.join(directory, 'bundle'), bundleLimits)) !== JSON.stringify(record.arms[arm].bundle)
      || JSON.stringify(inventory(path.join(directory, 'profile'))) !== JSON.stringify(record.arms[arm].profile))) throw new Error('prepared runtime bundle/profile changed');
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
    const result = spawnSync(record.runner.command, record.runner.args.map((arg) => arg.replaceAll('{workspace}', workspace)
      .replaceAll('{bundle}', path.join(directory, 'bundle')).replaceAll('{profile}', path.join(directory, 'profile'))), {
      cwd: workspace, input: prompt, encoding: 'utf8',
      timeout: record.runner.timeoutMs, maxBuffer: 2 * 1024 * 1024, shell: false
    });
    fs.writeFileSync(path.join(directory, 'stdout.txt'), result.stdout ?? '', { flag: 'wx' });
    fs.writeFileSync(path.join(directory, 'stderr.txt'), result.stderr ?? '', { flag: 'wx' });
    let after, profileAfter, bundleAfter, inspectionError = null;
    try {
      after = inventory(workspace);
      if (record.mode === 'isolated-bundles') {
        profileAfter = inventory(path.join(directory, 'profile'));
        bundleAfter = inventory(path.join(directory, 'bundle'), bundleLimits);
        if (JSON.stringify(bundleAfter) !== JSON.stringify(record.arms[arm].bundle)) inspectionError = 'runner changed its frozen runtime bundle';
      }
    } catch (error) { inspectionError = error.message; }
    const entry = { exitCode: result.status, signal: result.signal, error: result.error?.message ?? null,
      durationMs: Date.now() - started, responsePresent: Boolean(result.stdout?.trim()),
      workspaceAfter: after ?? null, inspectionError,
      ...(record.mode === 'isolated-bundles' ? { bundleIdentity: record.arms[arm].bundleIdentity, bundleAfter, profileAfter,
        nativeInstallation: 'runner-owned; not established by copying files or successful process exit' } : {}),
      tokenUsage: null, cost: null, semanticVerdict: 'not-judged' };
    writeJson(path.join(directory, 'result.json'), entry);
    results[arm] = entry;
    // An unavailable runner is a setup failure; do not repeat the same failing invocation.
    if (result.error || result.status !== 0 || !entry.responsePresent || entry.inspectionError) break;
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
