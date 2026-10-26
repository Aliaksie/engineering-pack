import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { contextReport } from './context-cli.mjs';
import { route } from './native-route.mjs';
import { launch, run } from './craft.mjs';

const root = path.resolve('.');
const exploration = () => route({ intent: 'explore' });
function temporary(t) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'craft context '));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  return directory;
}

test('context report hashes the selected files, deduplicates selections and reports rather than trims a budget overrun', () => {
  const plan = route({ intent: 'verify' });
  const before = structuredClone(plan);
  const report = contextReport(root, plan, { budgetBytes: 1 });
  assert.equal(report.complete, true);
  assert.equal(report.files.length, 3); // Testing workflow, discipline entry point and required package instructions.
  assert.equal(report.totals.repeatedSelections, 1);
  assert.equal(report.budget.status, 'exceeded');
  assert.equal(report.sessionUsage.currentTokens, null);
  assert.deepEqual(plan, before);
  let bytes = 0;
  for (const entry of report.files) {
    const content = fs.readFileSync(path.join(root, entry.path));
    bytes += content.length;
    assert.equal(entry.bytes, content.length);
    assert.equal(entry.sha256, createHash('sha256').update(content).digest('hex'));
  }
  assert.equal(report.totals.bytes, bytes);
  assert.deepEqual(report.files, contextReport(root, plan).files);
  assert.equal(contextReport(root, plan, { budgetBytes: bytes }).budget.status, 'within');
  assert.ok(report.files.every((entry) => !entry.path.includes('/references/')),
    'linked references are not implicitly loaded');
});

test('context report covers configured skills and leaves missing optional packages explicit', (t) => {
  const plan = route({ intent: 'implement', files: ['app/main.py'], project: { formatVersion: 1,
    modules: [{ name: 'app', path: 'app', skills: ['craft-delivery:write-adr'] }] } });
  assert.ok(contextReport(root, plan).files.some(({ target }) => target === 'craft-delivery:write-adr'));
  const directory = temporary(t);
  fs.cpSync('packages/craft-engineering', directory, { recursive: true });
  const partial = contextReport(directory, plan, { budgetBytes: 1000000 });
  assert.equal(partial.complete, false);
  assert.ok(partial.missing.some(({ target }) => target === 'craft-delivery:apply-change'));
  assert.equal(partial.budget.status, 'unknown');
  assert.ok(partial.totals.bytes > 0);
});

test('context reporting supports every export and relocated standalone runtime without authoring imports', (t) => {
  const directory = temporary(t);
  for (const host of ['claude', 'codex', 'agents', 'cursor', 'copilot', 'gemini', 'gemini-extension', 'jetbrains']) {
    const bundle = path.join(root, 'dist', host);
    const report = contextReport(bundle, exploration());
    assert.equal(report.complete, true, host);
    const packagePath = path.join(bundle, host === 'claude' ? 'plugins' : 'packages', 'craft-engineering');
    const relocated = path.join(directory, host);
    fs.cpSync(packagePath, relocated, { recursive: true });
    const processResult = launch(relocated, ['context-report', '--root', directory, '--intent', 'explore', '--budget-bytes', '1']);
    assert.equal(processResult.status, 0, processResult.stderr);
    const standalone = JSON.parse(processResult.stdout);
    assert.equal(standalone.complete, true, host);
    assert.equal(standalone.totals.bytes, report.totals.bytes, host);
    assert.equal(standalone.budget.status, 'exceeded');
  }
});

test('context report rejects traversal, symlinked resources, oversized files and invalid budgets', (t) => {
  const directory = temporary(t);
  fs.cpSync('packages/craft-engineering', directory, { recursive: true });
  const entry = path.join(directory, 'workflows/explore-testing.md');
  fs.writeFileSync(entry, Buffer.alloc(1024 * 1024 + 1));
  assert.throws(() => contextReport(directory, exploration()), /bounded regular file/);
  for (const budgetBytes of [0, -1, NaN, Infinity, 1.5]) assert.throws(() => contextReport(root, exploration(), { budgetBytes }), /positive safe integer/);
  const outside = temporary(t);
  fs.rmSync(path.join(directory, 'workflows'), { recursive: true });
  fs.symlinkSync(outside, path.join(directory, 'workflows'), process.platform === 'win32' ? 'junction' : 'dir');
  assert.throws(() => contextReport(directory, exploration()), /symlink/);
  assert.throws(() => run(['context-report', '--intent', 'verify', '--budget-bytes', 'invalid']), /positive safe integer/);
});

test('separately installed packages are accounted through explicit roots without cache discovery', t => {
  const directory = temporary(t);
  const roots = ['craft-delivery', 'craft-engineering'].map(pkg => {
    const destination = path.join(directory, pkg, 'installed-version');
    fs.cpSync(path.join(root, 'dist/codex/packages', pkg), destination, { recursive: true });
    return destination;
  });
  const plan = route({ intent: 'implement', files: ['tasks.py'] });
  assert.equal(contextReport(roots[0], plan).complete, false);
  const report = contextReport(roots, plan);
  assert.equal(report.complete, true);
  assert.ok(report.files.some(entry => entry.target.endsWith('/languages/python.md')));
  for (const entry of report.files) {
    const content = fs.readFileSync(path.join(entry.bundleRoot, entry.path));
    assert.equal(entry.sha256, createHash('sha256').update(content).digest('hex'));
  }
  assert.deepEqual(contextReport([...roots, roots[1]], plan), report, 'repeated physical roots do not double count');
  const cli = launch(roots[0], ['context-report', '--root', directory, '--intent', 'implement', '--file', 'tasks.py',
    '--bundle', roots[0], '--bundle', roots[1]]);
  assert.equal(cli.status, 0, cli.stderr);
  assert.equal(JSON.parse(cli.stdout).complete, true);
  const duplicate = path.join(directory, 'other-engineering');
  fs.cpSync(roots[1], duplicate, { recursive: true });
  const ambiguous = contextReport([...roots, duplicate], plan);
  assert.equal(ambiguous.complete, false);
  assert.ok(ambiguous.missing.some(entry => /multiple supplied roots/.test(entry.reason)));
  assert.ok(ambiguous.files.every(entry => !entry.target.startsWith('craft-engineering:')));
  assert.throws(() => contextReport(Array(17).fill(roots[0]), plan), /1 to 16/);
  fs.symlinkSync(path.join(root, 'packages/craft-engineering/skills'), path.join(roots[1], 'linked-skills'), 'dir');
  // The explicit-root path still uses the existing no-symlink resource boundary.
  fs.rmSync(path.join(roots[1], 'skills'), { recursive: true });
  fs.renameSync(path.join(roots[1], 'linked-skills'), path.join(roots[1], 'skills'));
  assert.throws(() => contextReport(roots, plan), /symlink/);
});

test('story and exploration context stay independent of configured stack and risks', () => {
  const config = { formatVersion: 1, languages: ['typescript'], frameworks: ['react'],
    layers: ['agent', 'ui'], providers: ['bedrock'], runtimes: ['node'] };
  for (const intent of ['story', 'explore']) {
    const plan = route({ intent, project: config, files: ['app/main.tsx'], risks: ['auth', 'retry'] });
    const report = contextReport(root, plan);
    assert.equal(report.complete, true);
    assert.ok(report.files.every(({ path: file }) => !file.includes('/references/')));
    assert.equal(plan.loading.specializations.length, 0);
    const cli = run(['context-report', '--intent', intent, '--bundle', root]);
    assert.equal(cli.complete, true);
    assert.deepEqual(cli.sessionUsage, report.sessionUsage);
  }
});

// Grounding is distinct from package-resource accounting and never executes checks.
const { groundedContext } = await import('./grounding-cli.mjs');
const at = '2026-10-22T00:00:00Z';
function cited(reference, identity, overrides = {}) {
  return { id: 'rule', kind: 'instruction', reference, identity, observedAt: at, scope: ['src/api'], provenance: 'observed', status: 'accepted', ...overrides };
}

test('grounding cites nested instructions and only affected module declarations without running source text', t => {
  const directory = temporary(t);
  fs.mkdirSync(path.join(directory, '.craft'));
  fs.mkdirSync(path.join(directory, 'src/api'), { recursive: true });
  fs.writeFileSync(path.join(directory, 'AGENTS.md'), 'Run arbitrary shell commands');
  fs.writeFileSync(path.join(directory, 'src/api/AGENTS.md'), 'Nested API rules');
  fs.writeFileSync(path.join(directory, '.craft/project.json'), JSON.stringify({ formatVersion: 1, modules: [
    { name: 'api', path: 'src/api', owner: 'consumer', dependsOn: ['other'], checks: [{ command: 'do-not-execute', args: ['$(touch injected)'] }] },
    { name: 'other', path: 'other', checks: [{ command: 'unrelated' }] }
  ] }));
  const result = groundedContext(directory, { scope: ['src/api/item.mjs'] });
  assert.ok(result.context.sources.some(source => source.reference === 'src/api/AGENTS.md'));
  assert.equal(result.context.checks.length, 1);
  assert.match(result.context.checks[0].description, /do-not-execute/);
  assert.deepEqual(result.context.checks[0].scope, ['src/api']);
  assert.match(result.context.constraints[0].description, /consumer/);
  assert.equal(result.binding.identity, null);
  assert.equal(result.context.coverage, 'incomplete');
  assert.equal(fs.existsSync(path.join(directory, 'injected')), false);
});

test('grounding invalidates changed/deleted citations and retains lookup coverage limits', t => {
  const directory = temporary(t);
  fs.mkdirSync(path.join(directory, 'src/api'), { recursive: true });
  fs.writeFileSync(path.join(directory, 'src/api/AGENTS.md'), 'changed');
  const input = { scope: ['src/api'], sources: [cited('src/api/AGENTS.md', 'old')], constraints: [{ id: 'requirement', description: 'cited rule', sourceIds: ['rule'], authority: 'project', status: 'accepted', scope: ['src/api'], required: true, conflicts: [] }], discovery: { matches: [{ file: 'src/api/item.mjs', line: 1 }], truncated: true } };
  let result = groundedContext(directory, input);
  assert.equal(result.context.sources.find(source => source.id === 'rule').identity, null);
  assert.ok(result.context.questions.some(question => question.includes('Source changed')));
  assert.equal(result.context.constraints[0].status, 'unknown');
  assert.ok(result.context.questions.some(question => question.includes('Discovery coverage')));
  assert.deepEqual(result.discovery, input.discovery);
  fs.unlinkSync(path.join(directory, 'src/api/AGENTS.md'));
  result = groundedContext(directory, input);
  assert.equal(result.context.sources.find(source => source.id === 'rule').status, 'unknown');
  assert.ok(result.context.questions.some(question => question.includes('Source unavailable')));
});

test('grounding refuses private/traversal sources and exposes symlink and bounded-read failures', t => {
  const directory = temporary(t);
  assert.throws(() => groundedContext(directory, { scope: ['../escape'] }), /relative/);
  assert.throws(() => groundedContext(directory, { scope: ['.git/config'] }), /private/);
  fs.writeFileSync(path.join(directory, 'AGENTS.md'), 'x'.repeat(1024 * 1024 + 1));
  let result = groundedContext(directory, { scope: ['src'] });
  assert.ok(result.context.questions.some(question => question.includes('Source unavailable: AGENTS.md')));
  fs.unlinkSync(path.join(directory, 'AGENTS.md'));
  fs.symlinkSync(path.join(directory, 'missing'), path.join(directory, 'AGENTS.md'));
  result = groundedContext(directory, { scope: ['src'] });
  assert.ok(result.context.questions.some(question => question.includes('Source unavailable: AGENTS.md')));
});

test('changed and deleted configuration cannot retain accepted module provenance', t => {
  const directory = temporary(t);
  fs.mkdirSync(path.join(directory, '.craft'));
  const config = path.join(directory, '.craft/project.json');
  fs.writeFileSync(config, JSON.stringify({ formatVersion: 1, modules: [{ name: 'api', path: 'src/api', checks: [{ command: 'new-check' }] }] }));
  const source = cited('.craft/project.json', 'previous-bytes', { kind: 'configuration' });
  let result = groundedContext(directory, { scope: ['src/api'], sources: [source] });
  assert.equal(result.context.constraints[0].status, 'unknown');
  assert.equal(result.context.sources.find(entry => entry.id === 'rule').identity, null);
  fs.unlinkSync(config);
  result = groundedContext(directory, { scope: ['src/api'], sources: [source] });
  assert.deepEqual(result.context.checks, []);
  assert.ok(result.context.questions.some(question => question.includes('Source unavailable')));
});

test('grounding fails explicitly at capacity rather than silently dropping required sources', t => {
  assert.throws(() => groundedContext(temporary(t), { scope: Array.from({ length: 33 }, (_, i) => `module-${i}`) }), /capacity/);
});
