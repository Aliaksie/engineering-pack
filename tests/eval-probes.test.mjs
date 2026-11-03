import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { MANIFEST, generate, taskFiles, validate } from '../tooling/eval-probes.mjs';

const grader = fileURLToPath(new URL('../evals/image/answer-grade.sh', import.meta.url));
const tools = ['bash', 'jq'].every(tool => spawnSync(tool, ['--version']).status === 0);
const manifest = JSON.parse(fs.readFileSync(MANIFEST, 'utf8'));

test('the probe manifest is valid and covers every claimed language and actor', () => {
  assert.deepEqual(validate(manifest), []);
  const languages = new Set(Object.values(manifest.repositories).map(entry => entry.language));
  for (const language of ['typescript', 'python', 'java', 'go', 'rust', 'docs']) assert.ok(languages.has(language), language);
  const actors = new Set(manifest.probes.map(probe => probe.actor));
  for (const actor of ['developer', 'reviewer', 'architect', 'qa']) assert.ok(actors.has(actor), actor);
  for (const name of Object.keys(manifest.repositories)) {
    assert.ok(manifest.probes.filter(probe => probe.repository === name).length >= 6, `${name} has at least six questions`);
  }
});

test('validation rejects unpinned sources, format mismatches and duplicates', () => {
  const problems = validate({
    formatVersion: 1,
    repositories: { r: { url: 'https://github.com/a/b', commit: 'main', language: 'go' } },
    probes: [
      { id: 'p', repository: 'r', actor: 'developer', kind: 'callers', format: 'symbol', question: 'q', expected: ['a.go'] },
      { id: 'p', repository: 'x', actor: 'pm', kind: 'k', format: 'path', question: 'q', expected: ['/abs.go'], allowed: ['/abs.go'] },
    ],
  });
  for (const fragment of ['commit must be a full SHA', 'does not match format symbol', 'duplicate id', 'unknown repository',
    'actor must be', 'is not relative', 'both expected and allowed']) {
    assert.ok(problems.some(problem => problem.includes(fragment)), fragment);
  }
});

test('a generated task pins the source commit, keeps the answer out of reach and replaces stale tasks', () => {
  const out = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-probes-'));
  try {
    fs.mkdirSync(path.join(out, 'repo-stale'));
    fs.mkdirSync(path.join(out, 'hand-written'));
    const names = generate(manifest, out);
    assert.equal(names.length, manifest.probes.length);
    assert.ok(!fs.existsSync(path.join(out, 'repo-stale')));
    assert.ok(fs.existsSync(path.join(out, 'hand-written')));
    const probe = manifest.probes[0];
    const source = manifest.repositories[probe.repository];
    const dir = path.join(out, `repo-${probe.id}`);
    const dockerfile = fs.readFileSync(path.join(dir, 'environment', 'Dockerfile'), 'utf8');
    assert.match(dockerfile, new RegExp(`fetch -q --depth 1 ${source.url} ${source.commit}`));
    // The ground truth lives only with the verifier and the reference solution.
    const visible = [fs.readFileSync(path.join(dir, 'instruction.md'), 'utf8'), dockerfile].join('\n');
    for (const cited of probe.expected) assert.ok(!visible.includes(cited), `${cited} leaks into what the agent sees`);
    assert.deepEqual(JSON.parse(fs.readFileSync(path.join(dir, 'tests', 'truth.json'), 'utf8')).expected, probe.expected);
    assert.match(fs.readFileSync(path.join(dir, 'task.toml'), 'utf8'), /answer_file = "ANSWER.md"/);
    assert.ok(fs.statSync(path.join(dir, 'tests', 'test.sh')).mode & 0o100);
  } finally { fs.rmSync(out, { recursive: true, force: true }); }
});

test('a reference answer with quotes and dollars survives the generated shell', () => {
  const { files } = taskFiles({ id: 'q', repository: 'r', actor: 'qa', kind: 'k', format: 'path', question: 'q',
    expected: ["it's/$Gson$Types.java"] }, { url: 'https://github.com/a/b', commit: 'a'.repeat(40), language: 'java' });
  const run = spawnSync('bash', ['-c', files['solution/solve.sh'].replace('/app/ANSWER.md', '/dev/stdout')], { encoding: 'utf8' });
  assert.equal(run.stdout, "it's/$Gson$Types.java\n");
});

function grade(truth, answer, files = []) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-grade-'));
  try {
    const repo = path.join(dir, 'repo');
    for (const file of files) {
      fs.mkdirSync(path.dirname(path.join(repo, file)), { recursive: true });
      fs.writeFileSync(path.join(repo, file), '');
    }
    fs.mkdirSync(repo, { recursive: true });
    if (answer !== null) fs.writeFileSync(path.join(repo, 'ANSWER.md'), answer);
    fs.writeFileSync(path.join(dir, 'truth.json'), JSON.stringify(truth));
    const run = spawnSync('bash', [grader, repo, path.join(dir, 'truth.json'), dir], { encoding: 'utf8' });
    assert.equal(run.status, 0, run.stderr);
    return JSON.parse(run.stdout);
  } finally { fs.rmSync(dir, { recursive: true, force: true }); }
}

const skip = !tools && 'needs bash and jq';
const adrs = ['docs/adr/0001-number-as-id.md', 'docs/adr/0002-slug-as-id.md'];

test('a path answer passes with list markers, line numbers and prose around it', { skip }, () => {
  const result = grade({ format: 'path', match: 'all', expected: ['docs/adr/0002-slug-as-id.md'] },
    'The current decision, e.g. per chi.Router docs:\n- `./docs/adr/0002-slug-as-id.md:3`\n', adrs);
  assert.deepEqual(result, { reward: 1, cited_found: 1, cited_expected: 1, cited_wrong: 0, invented_paths: 0 });
});

test('citing a superseded record as well fails the answer', { skip }, () => {
  const result = grade({ format: 'path', match: 'all', expected: ['docs/adr/0002-slug-as-id.md'] }, `${adrs.join('\n')}\n`, adrs);
  assert.equal(result.reward, 0);
  assert.equal(result.cited_wrong, 1);
});

test('an invented path fails even when it is allowed and the expected one is present', { skip }, () => {
  const result = grade({ format: 'path', match: 'any', expected: ['a/x.go'], allowed: ['a/gone.go'] }, 'a/x.go\na/gone.go\n', ['a/x.go']);
  assert.equal(result.invented_paths, 1);
  assert.equal(result.reward, 0);
});

test('match any accepts one expected citation plus allowed ones; match all needs every one', { skip }, () => {
  const files = ['a.go', 'b.go', 'c.go'];
  assert.equal(grade({ format: 'path', match: 'any', expected: ['a.go', 'b.go'], allowed: ['c.go'] }, 'a.go\nc.go\n', files).reward, 1);
  assert.equal(grade({ format: 'path', match: 'all', expected: ['a.go', 'b.go'] }, 'a.go\n', files).reward, 0);
});

test('symbol answers compare the last name segment and ignore plain paths', { skip }, () => {
  const truth = { format: 'symbol', match: 'all', expected: ['src/urls.py#set', 'src/urls.py#__init__'] };
  const result = grade(truth, 'src/urls.py\n- src/urls.py#QueryParams.set\n- `src/urls.py#QueryParams.__init__()`\n', ['src/urls.py']);
  assert.deepEqual(result, { reward: 1, cited_found: 2, cited_expected: 2, cited_wrong: 0, invented_paths: 0 });
});

test('a missing answer file scores zero', { skip }, () => {
  assert.equal(grade({ format: 'path', expected: ['a.go'] }, null, ['a.go']).reward, 0);
});
