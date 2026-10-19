import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import readline from 'node:readline';
import { spawn } from 'node:child_process';
import test from 'node:test';
import assert from 'node:assert/strict';

// Optional real-host check: no model calls, hook execution or trust writes.
// CRAFT_CODEX_SMOKE=1 node --test tests/codex-hook-discovery.test.mjs
test('Codex discovers every installed package hook without granting trust', {
  skip: process.env.CRAFT_CODEX_SMOKE !== '1', timeout: 30000
}, async t => {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-codex-discovery-'));
  const source = path.resolve('dist/codex');
  const marketplace = path.join(source, '.agents/plugins/marketplace.json');
  fs.writeFileSync(path.join(home, 'config.toml'),
    `[marketplaces.engineering-pack]\nsource_type = "local"\nsource = ${JSON.stringify(source)}\n`);
  const child = spawn('codex', ['app-server', '--stdio', '--disable', 'remote_plugin'], {
    env: { ...process.env, CODEX_HOME: home }, stdio: ['pipe', 'pipe', 'pipe']
  });
  const pending = new Map();
  let sequence = 0;
  let stderr = '';
  child.stderr.on('data', chunk => { stderr = (stderr + chunk).slice(-4096); });
  const lines = readline.createInterface({ input: child.stdout });
  const rejectPending = error => {
    for (const request of pending.values()) { clearTimeout(request.timer); request.reject(error); }
    pending.clear();
  };
  child.once('error', rejectPending);
  child.once('exit', code => rejectPending(new Error(`Codex exited ${code}: ${stderr}`)));
  t.after(async () => {
    lines.close();
    if (child.pid && child.exitCode === null && child.signalCode === null) {
      const exited = new Promise(resolve => child.once('exit', resolve));
      child.kill();
      await exited;
    }
    fs.rmSync(home, { recursive: true, force: true });
  });
  lines.on('line', line => {
    let message;
    try { message = JSON.parse(line); } catch { return; }
    const request = pending.get(message.id);
    if (!request) return;
    pending.delete(message.id);
    clearTimeout(request.timer);
    if (message.error) request.reject(new Error(JSON.stringify(message.error)));
    else request.resolve(message.result);
  });
  const call = (method, params) => new Promise((resolve, reject) => {
    const id = ++sequence;
    const timer = setTimeout(() => {
      pending.delete(id);
      reject(new Error(`Codex timed out during ${method}: ${stderr}`));
    }, 10000);
    pending.set(id, { resolve, reject, timer });
    child.stdin.write(`${JSON.stringify({ id, method, params })}\n`);
  });
  const initialized = await call('initialize', {
    clientInfo: { name: 'craft_hook_discovery_test', version: '0.0.1' },
    capabilities: { experimentalApi: true }
  });
  t.diagnostic(`Host: ${initialized.userAgent ?? 'Codex app-server'}`);
  const catalog = JSON.parse(fs.readFileSync(marketplace, 'utf8'));
  for (const plugin of catalog.plugins) {
    await call('plugin/install', { pluginName: plugin.name, marketplacePath: marketplace });
  }
  const listed = await call('hooks/list', { cwds: [process.cwd()] });
  assert.equal(listed.data.length, 1);
  const entry = listed.data[0];
  assert.deepEqual(entry.errors, []);
  assert.deepEqual(entry.warnings, []);
  assert.equal(new Set(entry.hooks.map(hook => hook.key)).size, entry.hooks.length);
  for (const plugin of catalog.plugins) {
    const manifest = JSON.parse(fs.readFileSync(path.join(source, plugin.source.path, 'source-capability.json'), 'utf8'));
    const detail = await call('plugin/read', { pluginName: plugin.name, marketplacePath: marketplace });
    assert.equal(detail.plugin.skills.length, manifest.skills.length + manifest.workflows.length);
    const hooks = entry.hooks.filter(hook => hook.pluginId === `${plugin.name}@${catalog.name}`);
    assert.equal(hooks.length, manifest.hooks.length, `${plugin.name}: real host discovery must match declared hooks`);
    assert.ok(hooks.length > 0);
    assert.ok(hooks.every(hook => hook.source === 'plugin' && hook.trustStatus === 'untrusted'));
    t.diagnostic(`${plugin.name}: ${hooks.length} hooks discovered, awaiting trust`);
  }
});
