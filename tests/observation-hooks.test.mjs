import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { copy } from '../tooling/export/export.mjs';
import { HOSTS } from '../hosts/registry.mjs';

const read = file => JSON.parse(fs.readFileSync(file, 'utf8'));
const events = root => {
  const file = path.join(root, '.capabilities/state/telemetry.jsonl');
  return fs.existsSync(file) ? fs.readFileSync(file, 'utf8').trim().split('\n').map(JSON.parse) : [];
};

for (const pkg of ['craft-engineering', 'craft-stories', 'craft-review']) for (const host of ['claude', 'codex', 'cursor', 'copilot', 'gemini']) {
  test(`${host}: registered standalone ${pkg} observational hooks record lifecycle without delivery effects`, t => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'engineering observation '));
    t.after(() => fs.rmSync(root, { recursive: true, force: true }));
    const output = path.resolve('dist', host);
    const packageRoot = path.join(root, `packages/${pkg}`);
    copy(path.join(output, host === 'claude' ? 'plugins' : 'packages', pkg), packageRoot);
    // Observation must work independently of delivery configuration and sibling packages.
    fs.mkdirSync(path.join(root, '.capabilities'));
    fs.writeFileSync(path.join(root, '.capabilities/project.json'), 'INVALID_PROJECT_MARKER');
    let config;
    if (host === 'claude') {
      const manifest = read(path.join(packageRoot, '.claude-plugin/plugin.json'));
      config = read(path.resolve(packageRoot, manifest.hooks));
    } else if (host === 'codex') {
      const manifest = read(path.join(packageRoot, '.codex-plugin/plugin.json'));
      assert.ok(!fs.existsSync(path.join(packageRoot, 'plugin.json')), 'portable manifest would mask native hook discovery');
      config = read(path.resolve(packageRoot, manifest.hooks));
    } else config = read(path.join(output, HOSTS[host].config));
    const definitions = read(path.join(packageRoot, 'source-capability.json')).hooks.filter(hook => hook.handler === 'hooks/observe.mjs');
    assert.equal(definitions.length, 7);
    for (const definition of definitions) {
      const mapping = HOSTS[host].events[definition.event];
      if (!mapping) continue;
      const handlers = config.hooks[mapping.name].flatMap(group => group.hooks ?? [group]);
      const matches = handlers.filter(handler => {
        const command = handler.command ?? handler.bash;
        return command.includes(`/${definition.name}.mjs`)
          && (['claude', 'codex'].includes(host) || command.includes(`/packages/${pkg}/`));
      });
      assert.equal(matches.length, 1, `${host}: missing or duplicate registration for ${definition.event}`);
      const command = matches[0].command ?? matches[0].bash;
      if (host === 'copilot') assert.equal(matches[0].powershell, command);
      const match = /^node "([^"]+)"$/.exec(command);
      assert.ok(match, command);
      const entry = match[1].replace('${CLAUDE_PLUGIN_ROOT}', packageRoot).replace('${PLUGIN_ROOT}', packageRoot);
      const result = spawnSync(process.execPath, [entry], {
        cwd: root, encoding: 'utf8', input: JSON.stringify({
          cwd: root, session_id: 'PRIVATE_SESSION_MARKER', tool_name: 'Read',
          prompt: 'PRIVATE_PROMPT_MARKER', tool_input: { file_path: 'PRIVATE_PATH_MARKER' }
        })
      });
      assert.equal(result.status, 0, result.stderr);
      assert.equal(result.stderr, '');
      assert.equal(result.stdout, '', 'observation must not emit permission decisions or context');
      const hook = events(root).filter(event => event.name === 'host.hook').at(-1);
      assert.equal(hook.attributes.lifecycle, definition.event);
      assert.equal(hook.host, host);
      assert.equal(hook.attributes.package, pkg);
      assert.equal(hook.attributes.inputStatus, 'complete');
      assert.equal(hook.outcome, 'success');
    }
    const recorded = events(root);
    assert.equal(recorded.filter(event => event.name === 'host.hook').length,
      definitions.filter(definition => HOSTS[host].events[definition.event]).length);
    assert.ok(recorded.some(event => event.name === 'host.observed'));
    assert.ok(recorded.every(event => ['host.hook', 'host.observed'].includes(event.name)));
    assert.ok(!JSON.stringify(recorded).includes('PRIVATE_'));
    assert.ok(!JSON.stringify(recorded).includes('INVALID_PROJECT_MARKER'));
    assert.deepEqual(fs.readdirSync(path.join(root, '.capabilities/state')).sort(),
      ['telemetry-key.json', 'telemetry.jsonl']);
    const beforeTool = path.join(packageRoot, 'platform-hooks/observe-before-tool.mjs');
    for (const input of ['INVALID_JSON_MARKER', JSON.stringify({
      cwd: root, sessionId: 'PRIVATE_SESSION_MARKER', toolName: 'write_file',
      toolArgs: { path: 'PRIVATE_PATH_MARKER', content: 'PRIVATE_CONTENT_MARKER'.repeat(4000) }
    })]) {
      const result = spawnSync(process.execPath, [beforeTool], { cwd: root, input, encoding: 'utf8' });
      assert.equal(result.status, 0, result.stderr);
      assert.equal(result.stdout, '');
      assert.equal(result.stderr, '');
    }
    const failures = events(root);
    assert.ok(failures.some(event => event.reasonCode === 'INPUT_TOO_LARGE'));
    const oversized = failures.find(event => event.reasonCode === 'INPUT_TOO_LARGE');
    assert.equal(oversized.attributes.package, pkg);
    assert.equal(oversized.attributes.lifecycle, 'before-tool');
    assert.equal(oversized.attributes.inputStatus, 'oversized');
    assert.equal(oversized.attributes.identityScope, 'invocation');
    assert.ok(failures.some(event => event.name === 'host.hook' && event.attributes.inputStatus === 'oversized'
      && event.attributes.identityScope === 'invocation'));
    assert.ok(failures.some(event => event.reasonCode === 'INVALID_INPUT'));
    assert.ok(failures.some(event => event.name === 'host.hook' && event.outcome === 'failure'));
    assert.ok(!JSON.stringify(failures).includes('PRIVATE_'));
    assert.ok(!JSON.stringify(failures).includes('INVALID_JSON_MARKER'));
    fs.writeFileSync(path.join(root, '.capabilities/telemetry.json'), '{"local":false}');
    const first = definitions[0];
    const before = fs.readFileSync(path.join(root, '.capabilities/state/telemetry.jsonl'), 'utf8');
    const off = spawnSync(process.execPath, [path.join(packageRoot, `platform-hooks/${first.name}.mjs`)], {
      cwd: root, encoding: 'utf8', input: JSON.stringify({ cwd: root, session_id: 'PRIVATE_SESSION_MARKER' })
    });
    assert.equal(off.status, 0, off.stderr);
    assert.equal(fs.readFileSync(path.join(root, '.capabilities/state/telemetry.jsonl'), 'utf8'), before);
  });
}

test('observational preparation and scope failures stay silent without invoking policy fallback', t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'observation failure '));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const runner = path.join(root, 'runner.mjs');
  const fallbackMarker = path.join(root, 'policy-fallback');
  const module = pathToFileURL(path.resolve('hosts/hooks.mjs')).href;
  fs.writeFileSync(runner, `import { runHook } from ${JSON.stringify(module)};
import fs from 'node:fs';
const fail = () => { throw new Error('PRIVATE_FAILURE_MARKER'); };
if (process.argv[2] === 'read') process.stdin[Symbol.asyncIterator] = async function* () { fail(); };
await runHook(fail, 'copilot', { event: 'before-tool', tool: 'any' }, process.argv[2] === 'evaluate' ? event => event : fail, {
  observational: true, default: () => ({ decision: 'allow' }),
  failure: () => { fs.writeFileSync(process.argv[3], 'called'); return { decision: 'deny' }; },
  ...(process.argv[2] === 'scope' ? { run: fail } : {})
});\n`);
  for (const phase of ['prepare', 'evaluate', 'scope', 'read']) {
    const result = spawnSync(process.execPath, [runner, phase, fallbackMarker], { input: '{}', encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout, '');
    assert.equal(result.stderr, '');
    assert.ok(!fs.existsSync(fallbackMarker));
  }
});

test('oversized inputs still fail existing policy hooks on every hook-capable adapter', t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'policy input bound '));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const input = JSON.stringify({ cwd: root, tool_name: 'Bash', tool_input: { command: 'x'.repeat(70000) } });
  for (const host of ['claude', 'codex', 'cursor', 'copilot', 'gemini']) {
    const entry = path.resolve('dist', host, host === 'claude' ? 'plugins' : 'packages',
      'craft-review/platform-hooks/read-only.mjs');
    const result = spawnSync(process.execPath, [entry], { cwd: root, input, encoding: 'utf8' });
    assert.equal(result.status, 1, `${host}: policy input rejection changed`);
    assert.match(result.stderr, /hook input exceeds 64 KiB/);
  }
});

test('guidance-only adapters report unsupported observational hooks without registering them', () => {
  for (const host of ['agents', 'gemini-extension', 'jetbrains']) {
    const output = path.resolve('dist', host);
    for (const report of read(path.join(output, 'adapter-report.json')).packages) {
      assert.match(report.hooks, /unsupported|guidance/);
      assert.ok(report.hookMappings.every(mapping => mapping.supported === false));
      assert.ok(!fs.existsSync(path.join(output, `packages/${report.package}/platform-hooks`)));
    }
  }
});
