import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import test from 'node:test';
import assert from 'node:assert/strict';
import { preparePair, runPair } from '../tooling/paired-eval.mjs';

function fixture(t, code = 'process.stdin.pipe(process.stdout)') {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'paired eval '));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.mkdirSync(path.join(root, 'fixture'));
  fs.writeFileSync(path.join(root, 'fixture/requirements.md'), 'Ordinary task evidence.\n');
  fs.writeFileSync(path.join(root, 'prompt.txt'), 'Common task.\n');
  fs.writeFileSync(path.join(root, 'guidance.md'), 'Candidate-only guidance.\n');
  const config = { fixture: 'fixture', prompt: 'prompt.txt', guidance: ['guidance.md'],
    runner: { command: process.execPath, args: ['-e', code], identity: 'deterministic-test-double; not a model', timeoutMs: 3000 } };
  const file = path.join(root, 'config.json'); fs.writeFileSync(file, JSON.stringify(config));
  return { root, file, output: path.join(root, 'results') };
}

test('paired collection freezes matched inputs, retains actual streams and never invents a model verdict', (t) => {
  const { root, file, output } = fixture(t);
  const record = preparePair(file, output, root);
  assert.equal(record.fixture.length, 1);
  assert.equal(fs.readFileSync(path.join(output, 'baseline/prompt.txt'), 'utf8'), 'Common task.\n');
  assert.match(fs.readFileSync(path.join(output, 'candidate/prompt.txt'), 'utf8'), /Common task[\s\S]*Candidate-only/);
  const result = runPair(output);
  assert.equal(result.collectionComplete, true);
  assert.equal(result.results.baseline.tokenUsage, null);
  assert.equal(result.results.candidate.semanticVerdict, 'not-judged');
  assert.match(fs.readFileSync(path.join(output, 'candidate/stdout.txt'), 'utf8'), /Candidate-only/);
  assert.throws(() => runPair(output), /EEXIST|already attempted/);
  assert.throws(() => preparePair(file, output, root), /EEXIST/);
});

test('failed collection preserves errors and does not repeat a failed setup for the other arm', (t) => {
  const { root, file, output } = fixture(t, 'process.stderr.write("setup failed"); process.exit(2)');
  preparePair(file, output, root);
  const result = runPair(output);
  assert.equal(result.collectionComplete, false);
  assert.equal(result.results.baseline.exitCode, 2);
  assert.equal(result.results.candidate, undefined);
  assert.equal(fs.existsSync(path.join(output, 'candidate/stdout.txt')), false);
  assert.match(fs.readFileSync(path.join(output, 'baseline/stderr.txt'), 'utf8'), /setup failed/);
});

test('tampered fixtures and prompts are rejected before runner execution', (t) => {
  for (const target of ['baseline/prompt.txt', 'candidate/workspace/requirements.md']) {
    const { root, file, output } = fixture(t);
    preparePair(file, output, root);
    fs.appendFileSync(path.join(output, target), 'changed');
    assert.throws(() => runPair(output), /input changed/);
    assert.equal(fs.existsSync(path.join(output, 'baseline/stdout.txt')), false);
  }
});

test('paired collection records explicit counterbalanced order and rejects invalid orders', (t) => {
  const { root, file, output } = fixture(t);
  const config = JSON.parse(fs.readFileSync(file, 'utf8'));
  config.order = ['candidate', 'baseline'];
  fs.writeFileSync(file, JSON.stringify(config));
  const record = preparePair(file, output, root);
  assert.deepEqual(record.order, ['candidate', 'baseline']);
  assert.deepEqual(Object.keys(runPair(output).results), ['candidate', 'baseline']);
  config.order = ['candidate', 'candidate']; fs.writeFileSync(file, JSON.stringify(config));
  assert.throws(() => preparePair(file, path.join(root, 'invalid'), root), /exactly once/);
});

test('a preceding runner cannot silently change the next arm inputs in either order', (t) => {
  for (const order of [['baseline', 'candidate'], ['candidate', 'baseline']]) {
    for (const target of ['prompt.txt', 'workspace/requirements.md']) {
      const code = `const fs = require('node:fs'); const path = require('node:path');
        fs.appendFileSync(path.resolve(process.cwd(), '../../${order[1]}/${target}'), 'changed by first arm');
        process.stdout.write('first arm completed');`;
      const { root, file, output } = fixture(t, code);
      const config = JSON.parse(fs.readFileSync(file, 'utf8'));
      config.order = order;
      fs.writeFileSync(file, JSON.stringify(config));
      preparePair(file, output, root);
      assert.throws(() => runPair(output), /input changed/);
      assert.equal(fs.readFileSync(path.join(output, order[0], 'stdout.txt'), 'utf8'), 'first arm completed');
      assert.equal(fs.existsSync(path.join(output, order[0], 'result.json')), true);
      assert.equal(fs.existsSync(path.join(output, order[1], 'stdout.txt')), false);
      assert.equal(fs.existsSync(path.join(output, 'collection.json')), false);
    }
  }
});

test('relative output paths pass an absolute workspace argv matching the runner cwd', t => {
  const { root, file, output } = fixture(t, 'process.stdout.write(JSON.stringify({cwd:process.cwd(),workspace:process.argv[1]}))');
  const config = JSON.parse(fs.readFileSync(file, 'utf8'));
  config.runner.args.push('{workspace}');
  fs.writeFileSync(file, JSON.stringify(config));
  const relative = path.relative(process.cwd(), output);
  preparePair(file, relative, root);
  assert.equal(runPair(relative).collectionComplete, true);
  for (const arm of ['baseline', 'candidate']) {
    const result = JSON.parse(fs.readFileSync(path.join(output, arm, 'stdout.txt'), 'utf8'));
    assert.equal(result.workspace, path.join(output, arm, 'workspace'));
    assert.equal(result.cwd, fs.realpathSync(result.workspace));
  }
});

function integratedFixture(t) {
  const f = fixture(t, `const fs=require('node:fs'),path=require('node:path');
const [bundle,profile]=process.argv.slice(1);
const runtime=fs.readFileSync(path.join(bundle,'runtime.txt'),'utf8');
const setting=fs.readFileSync(path.join(profile,'settings.json'),'utf8');
fs.writeFileSync(path.join(profile,'observed.txt'),runtime);
process.stdout.write(JSON.stringify({runtime,setting,bundle,profile}));`);
  const config = JSON.parse(fs.readFileSync(f.file, 'utf8')); delete config.guidance;
  config.bundles = { baseline: 'baseline-runtime', candidate: 'candidate-runtime' };
  for (const arm of ['baseline', 'candidate']) {
    fs.mkdirSync(path.join(f.root, `${arm}-runtime`));
    fs.writeFileSync(path.join(f.root, `${arm}-runtime/runtime.txt`), `${arm} actual bundle`);
    fs.writeFileSync(path.join(f.root, `${arm}-settings.json`), JSON.stringify({ arm }));
  }
  config.profileFiles = Object.fromEntries(['baseline', 'candidate'].map(arm => [arm, { 'settings.json': `${arm}-settings.json` }]));
  config.runner.args.push('{bundle}', '{profile}');
  fs.writeFileSync(f.file, JSON.stringify(config)); return f;
}

test('isolated mode freezes each actual bundle/profile and delivers identical natural-task prompts', t => {
  const { root, file, output } = integratedFixture(t);
  const record = preparePair(file, output, root);
  assert.equal(record.formatVersion, 2); assert.equal(record.mode, 'isolated-bundles');
  assert.equal(record.arms.baseline.promptSha256, record.arms.candidate.promptSha256);
  assert.notEqual(record.arms.baseline.bundleIdentity, record.arms.candidate.bundleIdentity);
  const result = runPair(output);
  assert.equal(result.collectionComplete, true);
  for (const arm of ['baseline', 'candidate']) {
    const observed = JSON.parse(fs.readFileSync(path.join(output, arm, 'stdout.txt'), 'utf8'));
    assert.equal(observed.runtime, `${arm} actual bundle`);
    assert.equal(JSON.parse(observed.setting).arm, arm);
    assert.equal(observed.profile, path.join(output, arm, 'profile'));
    assert.ok(result.results[arm].profileAfter.some(entry => entry.path === 'observed.txt'));
    assert.equal(result.results[arm].semanticVerdict, 'not-judged');
    assert.match(result.results[arm].nativeInstallation, /not established/);
  }
});

test('bundle/profile mutation in either arm is rejected before any runner; traversal and implicit profiles fail', t => {
  for (const target of ['candidate/bundle/runtime.txt', 'baseline/profile/settings.json']) {
    const { root, file, output } = integratedFixture(t);
    preparePair(file, output, root); fs.appendFileSync(path.join(output, target), 'tamper');
    assert.throws(() => runPair(output), /bundle\/profile changed/);
    assert.equal(fs.existsSync(path.join(output, 'baseline/stdout.txt')), false);
  }
  const { root, file, output } = integratedFixture(t);
  const config = JSON.parse(fs.readFileSync(file, 'utf8'));
  config.profileFiles.baseline = { '../escape.json': 'baseline-settings.json' };
  fs.writeFileSync(file, JSON.stringify(config));
  assert.throws(() => preparePair(file, output, root), /profile path/);
  delete config.profileFiles; config.runner.args = ['-e', 'process.stdout.write("unused")'];
  fs.writeFileSync(file, JSON.stringify(config));
  assert.throws(() => preparePair(file, output, root), /explicitly consume/);
});

test('the first runner cannot change the next frozen bundle or profile without detection', t => {
  const { root, file, output } = integratedFixture(t), config = JSON.parse(fs.readFileSync(file, 'utf8'));
  config.runner.args[1] = `const fs=require('node:fs'),path=require('node:path');fs.appendFileSync(path.resolve(process.cwd(),'../../candidate/bundle/runtime.txt'),'tamper');process.stdout.write('baseline completed');`;
  fs.writeFileSync(file, JSON.stringify(config)); preparePair(file, output, root);
  assert.throws(() => runPair(output), /bundle\/profile changed/);
  assert.equal(fs.existsSync(path.join(output, 'baseline/result.json')), true);
  assert.equal(fs.existsSync(path.join(output, 'candidate/stdout.txt')), false);
});

test('changing the executing bundle retains evidence and stops collection before the next arm', t => {
  const { root, file, output } = integratedFixture(t), config = JSON.parse(fs.readFileSync(file, 'utf8'));
  config.runner.args[1] = `const fs=require('node:fs'),path=require('node:path');fs.appendFileSync(path.join(process.argv[1],'runtime.txt'),'tamper');process.stdout.write('runtime changed');`;
  fs.writeFileSync(file, JSON.stringify(config)); preparePair(file, output, root);
  const result = runPair(output);
  assert.equal(result.collectionComplete, false);
  assert.match(result.results.baseline.inspectionError, /frozen runtime bundle/);
  assert.ok(result.results.baseline.bundleAfter.length);
  assert.equal(fs.existsSync(path.join(output, 'candidate/stdout.txt')), false);
});
