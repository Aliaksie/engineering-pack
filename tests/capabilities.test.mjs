import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { ADAPTERS, build, checkBuild } from '../tooling/build.mjs';
import { files, frontmatter, json, loadCapabilities, validateCapability } from '../tooling/lib/capabilities.mjs';
import { evaluateReport } from '../tooling/eval-report.mjs';
import { copy } from '../tooling/export/export.mjs';
import { HOSTS } from '../hosts/registry.mjs';
import { runHook } from './hook-command.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const packages = loadCapabilities(root);
const byName = (name) => packages.find(({ manifest }) => manifest.name === name);
const writeJson = (file, value) => fs.writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`);

function fixture(t) {
  const repo = fs.mkdtempSync(path.join(os.tmpdir(), 'capability test '));
  t.after(() => fs.rmSync(repo, { recursive: true, force: true }));
  copy(path.join(root, 'hosts'), path.join(repo, 'hosts'));
  copy(path.join(root, 'docs'), path.join(repo, 'docs'));
  for (const folder of ['engine', 'application', 'platform', 'integrations', 'cli']) copy(path.join(root, folder), path.join(repo, folder));
  fs.copyFileSync(path.join(root,'package.json'),path.join(repo,'package.json'));
  const source = byName('craft-review');
  const packageRoot = path.join(repo, 'packages', source.manifest.name);
  copy(source.root, packageRoot);
  const manifest = structuredClone(source.manifest);
  manifest.optionalDependencies = [];
  writeJson(path.join(packageRoot, 'capability.json'), manifest);
  return { repo, packageRoot, manifest };
}

test('canonical packages have portable skills, declared resources and evals', () => {
  assert.ok(packages.length === 4);
  for (const { root: directory, manifest } of packages) {
    assert.deepEqual(validateCapability(directory, manifest), []);
    assert.ok(manifest.evals.length, `${manifest.name} has no behavioral case`);
    assert.ok(!fs.existsSync(path.join(directory, '.claude-plugin')));
    for (const skill of manifest.skills) frontmatter(fs.readFileSync(path.join(directory, skill, 'SKILL.md'), 'utf8'));
  }
});

test('invalid paths, unknown fields and unsupported features are rejected before export', () => {
  const { root: directory, manifest } = byName('craft-delivery');
  for (const bad of [
    { ...manifest, surprise: true },
    { ...manifest, formatVersion: 999 },
    { ...manifest, standards: ['../README.md'] },
    { ...manifest, standards: ['/etc/passwd'] },
    { ...manifest, skills: [null] },
    { ...manifest, instructions: 'untracked-instructions.md' },
    { ...manifest, hooks: [{ name: 'unsupported', event: 'after-everything', tool: 'shell', handler: 'handler.mjs' }] },
    { ...manifest, mcp: [{ name: 'private', transport: 'http', url: 'https://example.com/mcp?token=secret' }] },
    { ...manifest, mcp: [{ name: 'secret', transport: 'stdio', command: 'server', env: { TOKEN: 'literal-secret' } }] },
    { ...manifest, mcp: [{ name: 'unknown', transport: 'http', url: 'https://example.com/mcp', auth: { type: 'magic' } }] }
  ]) assert.ok(validateCapability(directory, bad).length, JSON.stringify(bad));
});

test('unregistered skills and symlinked assets cannot be shipped', (t) => {
  const { packageRoot, manifest } = fixture(t);
  assert.ok(validateCapability(packageRoot, { ...manifest, skills: [] }).some((error) => error.includes('exactly')));
  fs.symlinkSync(path.join(root, 'README.md'), path.join(packageRoot, 'external.md'));
  assert.ok(validateCapability(packageRoot, manifest).some((error) => /symlink/.test(error)));
});

test('all adapters preserve resources, separate names, and report actual feature coverage', () => {
  for (const target of ['claude', 'codex', 'cursor', 'copilot', 'gemini', 'agents']) {
    assert.deepEqual(checkBuild(root, target), []);
    const exportRoot = path.join(root, 'dist', target);
    const license = fs.readFileSync(path.join(root, 'LICENSE'), 'utf8');
    assert.equal(fs.readFileSync(path.join(exportRoot, 'LICENSE'), 'utf8'), license);
    const report = json(path.join(exportRoot, 'adapter-report.json'));
    assert.equal(report.packages.length, packages.length);
    for (const { manifest } of packages) {
      const entry = report.packages.find((entry) => entry.package === manifest.name);
      const directory = path.join(exportRoot, target === 'claude' ? 'plugins' : 'packages', manifest.name);
      assert.equal(fs.readFileSync(path.join(directory, 'LICENSE'), 'utf8'), license);
      assert.equal(entry.evals, 'included-not-executed');
      const setup = json(path.join(directory, 'SETUP.json'));
      assert.equal(setup.adapter, target);
      assert.equal(setup.hooks, entry.hooks);
      assert.ok(fs.existsSync(path.join(directory, setup.sessionAdvice.entry)));
      const setupPage = fs.readFileSync(path.join(directory, 'SETUP.md'), 'utf8');
      assert.ok(setupPage.startsWith(fs.readFileSync(path.join(root, 'packages/craft-engineering/standards/session-setup.md'), 'utf8')));
      const notice = JSON.parse(setupPage.match(/```json\n([\s\S]+?)\n```/)[1]);
      assert.deepEqual(notice, { adapter: target, hooks: setup.hooks, sessionAdvice: setup.sessionAdvice,
        controls: { surface: setup.controls.surface, commands: setup.controls.commands } });
      if (target !== 'codex') assert.deepEqual(setup.controls.commands, {});
      for (const asset of [...manifest.standards, ...manifest.evals, ...manifest.tools.map((tool) => tool.runtime === 'native' ? 'bin/craft' : tool.path)]) {
        assert.ok(fs.existsSync(path.join(directory, asset)), `${target}: missing ${asset}`);
      }
      for (const name of entry.skills) {
        const location = HOSTS[target].skills ? path.join(exportRoot, HOSTS[target].skills, name) : path.join(directory, 'skills', name);
        const info = frontmatter(fs.readFileSync(path.join(location, 'SKILL.md'), 'utf8'));
        assert.equal(info.name, name);
        const sourceSkill = manifest.skills.find((skill) => name === (target === 'claude' ? path.basename(skill) : `${manifest.name}-${path.basename(skill)}`));
        if (sourceSkill && fs.existsSync(path.join(root, 'packages', manifest.name, sourceSkill, 'references'))) {
          assert.ok(fs.existsSync(path.join(location, 'references')), `${target}: lost references for ${name}`);
        }
        const instructions = /\[package instructions\]\(([^)]+)\)/.exec(info.body)?.[1];
        assert.ok(instructions && fs.existsSync(path.resolve(location, instructions)), `${target}: missing instructions for ${name}`);
      }
      if (manifest.hooks.length) {
        assert.match(entry.hooks, target === 'agents' ? /unsupported/ : /native/);
        assert.equal(fs.existsSync(path.join(directory, 'platform-hooks/hooks.json')), ['claude', 'codex'].includes(target));
      }
    }
    if (target === 'agents') assert.ok(!files(exportRoot).some((file) => path.basename(file) === 'hooks.json'));
  }
});

test('generated host exports run after relocation without source-tree imports', (t) => {
  const { repo } = fixture(t);
  fs.mkdirSync(path.join(repo, '.craft'));
  writeJson(path.join(repo, '.craft/project.json'), { formatVersion: 1, review: { readOnly: true } });
  for (const target of ['claude', 'codex']) {
    build(repo, target);
    const moved = path.join(repo, `relocated ${target}`);
    fs.renameSync(path.join(repo, 'dist', target), moved);
    const standalone = path.join(moved, target === 'claude' ? 'plugins' : 'packages', 'craft-review');
    const setup = json(path.join(standalone, 'SETUP.json'));
    assert.ok(fs.existsSync(path.join(standalone, 'SETUP.md')));
    assert.ok(fs.existsSync(path.join(standalone, setup.sessionAdvice.entry)));
    const result = runHook(standalone, target, 'read-only', { cwd: repo, input: JSON.stringify({ cwd: repo, tool_name: 'Bash', tool_input: { command: 'gh pr comment 1 --body example' } }) });
    if (target === 'claude') { assert.equal(result.status, 2); assert.match(result.stderr, /read-only review/); }
    else { assert.equal(result.status, 0); assert.equal(JSON.parse(result.stdout).hookSpecificOutput.permissionDecision, 'deny'); }
  }
});

test('MCP exports retain external credentials and map transport names without activating servers', (t) => {
  const { repo, packageRoot, manifest } = fixture(t);
  manifest.mcp = [
    { name: 'docs', transport: 'http', url: 'https://example.com/mcp' },
    { name: 'local', transport: 'stdio', command: 'server-on-path', args: ['--read-only'], env: { SERVICE_TOKEN: '${SERVICE_TOKEN}' } }
  ];
  writeJson(path.join(packageRoot, 'capability.json'), manifest);
  for (const target of ['claude', 'codex', 'cursor', 'copilot', 'gemini', 'agents']) build(repo, target);
  const claude = json(path.join(repo, 'dist/claude/plugins/craft-review/.mcp.json'));
  const codexRoot = path.join(repo, 'dist/codex/packages/craft-review');
  const codexManifest = json(path.join(codexRoot, '.codex-plugin/plugin.json'));
  const codex = json(path.resolve(codexRoot, codexManifest.mcpServers));
  assert.equal(claude.mcpServers.docs.type, 'http');
  assert.equal(codex.mcpServers.docs.type, 'streamable-http');
  assert.equal(codex.mcpServers.local.env.SERVICE_TOKEN, '${SERVICE_TOKEN}');
  const report = json(path.join(repo, 'dist/agents/adapter-report.json'));
  assert.equal(report.packages[0].mcp, 'declarations-only-not-registered');
});

test('builds are reproducible and refuse unmanaged or symlinked output', (t) => {
  const { repo } = fixture(t);
  build(repo, 'agents');
  const output = path.join(repo, 'dist/agents');
  const before = files(output).map((file) => [path.relative(output, file), fs.readFileSync(file, 'utf8')]);
  build(repo, 'agents');
  assert.deepEqual(files(output).map((file) => [path.relative(output, file), fs.readFileSync(file, 'utf8')]), before);
  fs.unlinkSync(path.join(output, '.capability-build.json'));
  assert.throws(() => build(repo, 'agents'), /unmanaged/);
  assert.equal(fs.readFileSync(path.join(output, 'AGENTS.md'), 'utf8'), before.find(([file]) => file === 'AGENTS.md')[1]);
  assert.throws(() => build(repo, 'unknown-platform'), /unknown adapter/);
  fs.rmSync(output, { recursive: true });
  fs.symlinkSync(path.join(repo, 'docs'), output, process.platform === 'win32' ? 'junction' : 'dir');
  assert.throws(() => build(repo, 'agents'), /symlink/);
});

function resultFixture() {
  const item = byName('craft-engineering');
  const scenario = json(path.join(item.root, item.manifest.evals[0]));
  return {
    package: item.manifest.name, packageVersion: item.manifest.version, case: scenario.id,
    adapter: 'codex', model: 'fixture-model', revision: 'fixture-revision', evaluator: 'fixture-reviewer',
    response: 'Synthetic test response, not a real model result.',
    verdicts: scenario.criteria.map((entry) => ({ criterion: entry.id, passed: true, evidence: 'Synthetic criterion evidence for validator tests.' }))
  };
}

test('eval summaries require complete evidence and never turn a recorded failure into a pass', () => {
  const report = resultFixture();
  assert.equal(evaluateReport(packages, report).success, true);
  report.verdicts[0].passed = false;
  assert.equal(evaluateReport(packages, report).success, false);
  for (const invalid of [
    { ...report, response: '' }, { ...report, evaluator: '' }, { ...report, packageVersion: '0.0.0' },
    { ...report, verdicts: report.verdicts.slice(1) }, { ...report, verdicts: [...report.verdicts, report.verdicts[0]] },
    { ...report, verdicts: report.verdicts.map((entry) => ({ ...entry, evidence: '' })) },
    { ...report, verdicts: report.verdicts.map((entry) => ({ ...entry, passed: 'true' })) }
  ]) assert.throws(() => evaluateReport(packages, invalid));
});

test('eval reports accept every exported host and reject unknown adapters', () => {
  for (const adapter of Object.keys(ADAPTERS)) {
    const report = { ...resultFixture(), adapter };
    assert.equal(evaluateReport(packages, report).adapter, adapter);
    report.verdicts[0].passed = false;
    assert.equal(evaluateReport(packages, report).success, false);
  }
  assert.throws(() => evaluateReport(packages, { ...resultFixture(), adapter: 'unknown' }), /unknown eval adapter/);
});
