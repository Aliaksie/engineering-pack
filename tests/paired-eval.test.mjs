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
