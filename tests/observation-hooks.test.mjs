import { stateFile } from '../platform/index.mjs';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { copy } from '../tooling/export/export.mjs';
import { HOSTS } from '../hosts/registry.mjs';
import { hookPackage, runHook } from './hook-command.mjs';

const read = file => JSON.parse(fs.readFileSync(file, 'utf8'));
const events = root => {
  const file = stateFile(root, 'telemetry.jsonl');
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
    fs.mkdirSync(path.join(root, '.craft'));
    fs.writeFileSync(path.join(root, '.craft/project.json'), 'INVALID_PROJECT_MARKER');
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
      const expected = `hook ${host} ${definition.event} --package ${pkg} --handler observe --scope any`;
      const matches = handlers.filter(handler => (handler.command ?? handler.bash).endsWith(` ${expected}`)
        && (['claude', 'codex'].includes(host) || (handler.command ?? handler.bash).includes(`/packages/${pkg}/`)));
      assert.equal(matches.length, 1, `${host}: missing or duplicate registration for ${definition.event}`);
      const command = matches[0].command ?? matches[0].bash;
      if (host === 'copilot') assert.equal(matches[0].powershell, `& ${command.replace('/bin/craft"', '/bin/craft.cmd"')}`);
      // The registered command exactly: the launcher (quoted except for Gemini's PowerShell-safe
      // relative path) and plain arguments, no Node.js.
      const match = (host === 'gemini' ? /^(\.\/packages\/[a-z0-9-]+\/bin\/craft) (hook .+)$/ : /^"([^"]+)" (hook .+)$/).exec(command);
      assert.ok(match, command);
      const launcher = match[1].replace('${CLAUDE_PLUGIN_ROOT}', packageRoot).replace('${PLUGIN_ROOT}', packageRoot).replace(/^\.\/packages\//, `${root}/packages/`);
      const result = spawnSync(process.platform === 'win32' ? `"${launcher}.cmd"` : launcher, match[2].split(' '), {
        cwd: root, encoding: 'utf8', shell: process.platform === 'win32', input: JSON.stringify({
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
    assert.deepEqual(fs.readdirSync(path.dirname(stateFile(root, 'probe.json'))).sort(),
      ['telemetry-key.json', 'telemetry.jsonl']);
    for (const input of ['INVALID_JSON_MARKER', JSON.stringify({
      cwd: root, sessionId: 'PRIVATE_SESSION_MARKER', toolName: 'write_file',
      toolArgs: { path: 'PRIVATE_PATH_MARKER', content: 'PRIVATE_CONTENT_MARKER'.repeat(4000) }
    })]) {
      const result = runHook(packageRoot, host, 'observe-before-tool', { cwd: root, input });
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
    fs.writeFileSync(path.join(root, '.craft/telemetry.json'), '{"local":false}');
    const first = definitions[0];
    const before = fs.readFileSync(stateFile(root, 'telemetry.jsonl'), 'utf8');
    const off = runHook(packageRoot, host, first.name, { cwd: root, input: JSON.stringify({ cwd: root, session_id: 'PRIVATE_SESSION_MARKER' }) });
    assert.equal(off.status, 0, off.stderr);
    assert.equal(fs.readFileSync(stateFile(root, 'telemetry.jsonl'), 'utf8'), before);
  });
}

test('oversized inputs still fail existing policy hooks on every hook-capable adapter', t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'policy input bound '));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const input = JSON.stringify({ cwd: root, tool_name: 'Bash', tool_input: { command: 'x'.repeat(70000) } });
  for (const host of ['claude', 'codex', 'cursor', 'copilot', 'gemini']) {
    const result = runHook(hookPackage(path.resolve('dist', host), host, 'craft-review'), host, 'read-only', { cwd: root, input });
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
      assert.ok(!fs.existsSync(path.join(output, `packages/${report.package}/bin/craft`)), 'no hook launcher without registered hooks');
    }
  }
});
