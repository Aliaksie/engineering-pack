import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { HOSTS } from '../hosts/registry.mjs';
import { saveCheckpoint } from './checkpoint-cli.mjs';
import { hookPackage, runHook } from './hook-command.mjs';

// Every source hook on every hook-capable host: registered as its table declares, answered in
// the host's wire format, and recorded the same way. Host differences come only from hooks.json.
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const native = ['claude', 'codex', 'cursor', 'copilot', 'gemini'];
const packages = fs.readdirSync(path.join(root, 'packages')).filter(name =>
  fs.existsSync(path.join(root, 'packages', name, 'capability.json')));
const sources = packages.flatMap(pkg =>
  (JSON.parse(fs.readFileSync(path.join(root, 'packages', pkg, 'capability.json'), 'utf8')).hooks ?? [])
    .map(hook => ({ ...hook, package: pkg })));
const order = ['session-start', 'before-prompt', 'before-tool', 'after-tool', 'before-compact', 'after-compact', 'turn-end', 'agent-start', 'after-tool-failure'];

const read = file => JSON.parse(fs.readFileSync(file, 'utf8'));
const configs = {
  claude: () => packages.map(pkg => read(path.join(root, 'dist/claude/plugins', pkg, 'platform-hooks/hooks.json'))),
  codex: () => packages.map(pkg => read(path.join(root, 'dist/codex/packages', pkg, 'platform-hooks/hooks.json'))),
  cursor: () => [read(path.join(root, 'dist/cursor/.cursor/hooks.json'))],
  copilot: () => [read(path.join(root, 'dist/copilot/.github/hooks/craft-capabilities.json'))],
  gemini: () => [read(path.join(root, 'dist/gemini/.gemini/settings.json'))],
};

// Registered commands as { name, matcher, host, lifecycle, package, handler, scope }.
function registered(host) {
  const entries = [];
  for (const config of configs[host]()) for (const [name, groups] of Object.entries(config.hooks ?? {})) for (const group of groups) {
    for (const hook of group.hooks ?? [group]) {
      const command = hook.command ?? hook.bash;
      const match = / hook (\S+) (\S+) --package (\S+) --handler (\S+) --scope (\S+)$/.exec(command);
      assert.ok(match, `${host} ${name}: unrecognized command ${command}`);
      entries.push({ name, matcher: group.matcher, host: match[1], lifecycle: match[2], package: match[3], handler: match[4], scope: match[5] });
    }
  }
  return entries;
}

function fixture(t, config = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'host conformance '));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  fs.mkdirSync(path.join(dir, '.craft'));
  fs.writeFileSync(path.join(dir, '.craft/project.json'), JSON.stringify({ formatVersion: 1, ...config }));
  return dir;
}

// A native payload for one lifecycle event, with each host's own session and tool fields.
function payload(host, lifecycle, cwd, session) {
  const id = host === 'cursor' ? { conversation_id: session } : host === 'copilot' ? { sessionId: session } : { session_id: session };
  const base = { ...id, cwd };
  if (lifecycle === 'before-prompt') return { ...base, prompt: 'review this change' };
  if (lifecycle === 'session-start') return { ...base, source: 'startup' };
  if (lifecycle === 'after-compact') return { ...base, source: 'compact' };
  if (lifecycle === 'before-compact') return { ...base, trigger: 'auto' };
  if (lifecycle !== 'before-tool' && lifecycle !== 'after-tool') return base;
  if (host === 'copilot') return { ...base, toolName: 'bash', toolArgs: { command: 'git status --short' } };
  const tool = { claude: 'Bash', codex: 'Bash', cursor: 'Shell', gemini: 'run_shell_command' }[host];
  return { ...base, tool_name: tool, tool_input: { command: 'git status --short' } };
}

function invoke(host, hook, cwd, session) {
  return runHook(hookPackage(path.join(root, 'dist', host), host, hook.package), host, hook.name, {
    cwd, input: JSON.stringify(payload(host, hook.event, cwd, session)),
  });
}

function wire(result) {
  return result.stdout.trim() ? JSON.parse(result.stdout) : {};
}
function contextOf(output) {
  return output.hookSpecificOutput?.additionalContext ?? output.additionalContext ?? output.additional_context ?? '';
}

// State record paths with the per-session directory and generated identities removed.
function records(dir) {
  const base = path.join(dir, '.craft', 'state');
  const walk = folder => fs.readdirSync(folder, { withFileTypes: true }).flatMap(entry =>
    entry.isDirectory() ? walk(path.join(folder, entry.name)) : [path.relative(base, path.join(folder, entry.name)).split(path.sep).join('/')]);
  return (fs.existsSync(base) ? walk(base) : [])
    .filter(name => !name.startsWith('telemetry/'))
    .map(name => name.replace(/^sessions\/[0-9a-f]{24}\//, 'sessions/<id>/').replace(/[0-9a-f]{16,}/g, '<hash>'))
    .sort();
}

test('every source hook is registered exactly where its host table maps the event', () => {
  for (const host of native) {
    const entries = registered(host);
    for (const hook of sources) {
      const mapping = HOSTS[host].events[hook.event];
      const scope = hook.tool === 'shell' ? 'shell' : 'any';
      const found = entries.filter(entry => entry.package === hook.package && entry.lifecycle === hook.event && entry.handler === hook.handler && entry.scope === scope);
      const label = `${host} ${hook.package}:${hook.name}`;
      if (!mapping) {
        assert.equal(found.length, 0, `${label}: unmapped event must not be registered`);
        continue;
      }
      assert.equal(found.length, 1, `${label}: expected one registration`);
      assert.equal(found[0].host, host, label);
      assert.equal(found[0].name, mapping.name, label);
      // Hosts with grouped matchers filter shell-scoped hooks before launch; others filter in the handler.
      const grouped = host === 'claude' || host === 'codex';
      assert.equal(found[0].matcher, mapping.matcher ?? (grouped && scope === 'shell' ? '^Bash$' : undefined), label);
    }
    for (const entry of entries) {
      assert.ok(sources.some(hook => hook.package === entry.package && hook.event === entry.lifecycle && hook.handler === entry.handler), `${host}: registration without a source hook ${JSON.stringify(entry)}`);
    }
  }
});

// Copilot CLI reads `.claude/settings.json` and `.claude/settings.local.json` and runs every
// hook from every source; Cursor loads Claude hooks too. Craft hooks in those files would run
// twice next to the overlay, so no export writes any there.
test('no export registers hooks in the Claude settings files other hosts also load', () => {
  const files = [];
  const walk = folder => {
    for (const entry of fs.readdirSync(folder, { withFileTypes: true })) {
      const file = path.join(folder, entry.name);
      if (entry.isDirectory() && entry.name !== 'native-bin') walk(file);
      else if (/^settings(?:\.local)?\.json$/.test(entry.name) && path.basename(folder) === '.claude') files.push(file);
    }
  };
  for (const host of [...native, 'agents', 'jetbrains']) walk(path.join(root, 'dist', host));
  for (const file of files) assert.equal(read(file).hooks, undefined, path.relative(root, file));
});

test('the adapter report lists each source hook with its host support', () => {
  for (const host of native) {
    const report = read(path.join(root, 'dist', host, 'adapter-report.json'));
    for (const hook of sources) {
      const mapping = report.packages.find(entry => entry.package === hook.package).hookMappings.find(entry => entry.id === hook.name);
      assert.ok(mapping, `${host} ${hook.package}:${hook.name} missing from the report`);
      assert.equal(mapping.supported !== false, Boolean(HOSTS[host].events[hook.event]), `${host} ${hook.package}:${hook.name}`);
    }
  }
});

test('every supported hook answers in its host wire format and injects context only where declared', (t) => {
  const dir = fixture(t);
  for (const host of native) for (const hook of sources) {
    const mapping = HOSTS[host].events[hook.event];
    if (!mapping) continue;
    const label = `${host} ${hook.package}:${hook.name}`;
    const result = invoke(host, hook, dir, `wire-${host}`);
    assert.equal(result.status, 0, `${label}: ${result.stderr}`);
    const output = wire(result);
    assert.equal(typeof output, 'object', label);
    if (output.hookSpecificOutput) assert.equal(output.hookSpecificOutput.hookEventName, mapping.name, label);
    if (!mapping.context) assert.equal(contextOf(output), '', `${label}: context on an event that cannot carry it`);
    if (output.decision === 'deny' || output.permission === 'deny' || output.permissionDecision === 'deny' || output.hookSpecificOutput?.permissionDecision === 'deny') {
      assert.fail(`${label}: a read-only command on an unrestricted project was denied`);
    }
  }
});

test('session start restores the same checkpoint and session identity on every host', (t) => {
  const dir = fixture(t, { checkpoint: { enabled: true } });
  const restore = sources.find(hook => hook.package === 'craft-delivery' && hook.event === 'session-start');
  for (const host of native) {
    const session = `restore-${host}`;
    saveCheckpoint(dir, session, { task: 'Conformance task', authorization: 'local review', nextStep: 'compare hosts' });
    const result = invoke(host, restore, dir, session);
    assert.equal(result.status, 0, `${host}: ${result.stderr}`);
    const context = contextOf(wire(result));
    assert.equal(Boolean(context), HOSTS[host].events['session-start'].context, host);
    if (!context) continue;
    assert.match(context, /^Craft session \(pass as --session; no authorization implied\): \{"id":"[^"]+"/, host);
    assert.match(context, /Conformance task/, host);
  }
});

// The same events in the same order, limited to lifecycle events every host maps.
function sequence(t, host) {
  const dir = fixture(t, { checkpoint: { enabled: true } });
  for (const lifecycle of order.filter(name => native.every(other => HOSTS[other].events[name]))) {
    for (const hook of sources.filter(entry => entry.event === lifecycle)) {
      const result = invoke(host, hook, dir, 'sequence-session');
      assert.equal(result.status, 0, `${host} ${hook.package}:${hook.name}: ${result.stderr}`);
    }
  }
  const log = path.join(dir, '.craft/state/telemetry/events.jsonl');
  const telemetry = fs.existsSync(log) ? fs.readFileSync(log, 'utf8').trim().split('\n').map(line => JSON.parse(line)) : [];
  return { records: records(dir), telemetry };
}
const signature = event => `${event.attributes?.package} ${event.name} ${event.outcome} ${event.reasonCode ?? ''} ${Object.keys(event.attributes ?? {}).sort()}`;
// Records and events that exist only because a host can show routing hints to the model.
const delivered = { records: /\/routing-<hash>\.json$/, telemetry: /^craft-delivery (?:cache\.observed|routing\.completed|guidance\.selected) / };

test('one lifecycle sequence records the same state and telemetry on every host', (t) => {
  const runs = Object.fromEntries(native.map(host => [host, sequence(t, host)]));
  const portable = host => ({
    records: runs[host].records.filter(name => !delivered.records.test(name)),
    telemetry: runs[host].telemetry.map(signature).filter(line => !delivered.telemetry.test(line)).sort(),
  });
  for (const host of native) {
    assert.deepEqual(portable(host), portable(native[0]), `${host} differs from ${native[0]}`);
    const hinted = HOSTS[host].events['before-prompt'].context;
    assert.equal(runs[host].records.some(name => delivered.records.test(name)), hinted, `${host}: routing record follows prompt context`);
    assert.equal(runs[host].telemetry.map(signature).some(line => delivered.telemetry.test(line)), hinted, `${host}: routing telemetry follows prompt context`);
  }
});

test('a declared host limit is not recorded as a failure', (t) => {
  for (const host of native) {
    const failures = sequence(t, host).telemetry.filter(event => event.outcome === 'failure').map(signature);
    assert.deepEqual(failures, [], host);
  }
});
