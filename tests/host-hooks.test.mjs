import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { HOSTS } from '../hosts/registry.mjs';
import { saveCheckpoint, readCheckpoint } from './checkpoint-cli.mjs';
import { hookPackage, registeredHook, runHook } from './hook-command.mjs';

// Records below the state directory: `<family>/<name>` or `sessions/<id>/<name>`.
const stateNames = root => {
  const base = path.join(root, '.craft', 'state');
  const walk = folder => fs.readdirSync(folder, { withFileTypes: true }).flatMap(entry =>
    entry.isDirectory() ? walk(path.join(folder, entry.name)) : [path.relative(base, path.join(folder, entry.name)).split(path.sep).join('/')]);
  return fs.existsSync(base) ? walk(base) : [];
};
const session = /^sessions\/[0-9a-f]{24}\//;
const only = (names, allowed) => names.every(name => name.startsWith('telemetry/') || allowed.test(name.replace(session, '')) && session.test(name));

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const native = ['claude', 'codex', 'cursor', 'copilot', 'gemini'];
function fixture(t, config = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'host hooks '));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  fs.mkdirSync(path.join(dir, '.craft'));
  fs.writeFileSync(path.join(dir, '.craft/project.json'), JSON.stringify({ formatVersion: 1, ...config }));
  return dir;
}
function payload(host, cwd, command, tool = 'shell') {
  if (host === 'copilot') return { sessionId: 'fixture-session', cwd, toolName: tool === 'shell' ? 'bash' : 'edit', toolArgs: { command, path: 'orders.py' } };
  return { session_id: 'fixture-session', cwd, tool_name: tool === 'shell' ? host === 'cursor' ? 'Shell' : host === 'gemini' ? 'run_shell_command' : 'Bash' : 'Write', tool_input: { command, file_path: 'orders.py' } };
}
function invoke(host, pkg, hook, input, cwd) {
  return runHook(hookPackage(path.join(root, 'dist', host), host, pkg), host, hook, { cwd, input: typeof input === 'string' ? input : JSON.stringify(input) });
}
function decision(host, result) {
  if (host === 'claude') return result.status === 2 ? 'deny' : 'allow';
  const output = result.stdout.trim() ? JSON.parse(result.stdout) : {};
  return output.hookSpecificOutput?.permissionDecision ?? output.permissionDecision ?? output.permission ?? output.decision ?? 'allow';
}

test('all native wrappers map read-only review denials without imposing a global restriction', (t) => {
  const guarded = fixture(t, { review: { readOnly: true } });
  const unrestricted = fixture(t);
  for (const host of native) {
    for (const command of ['gh pr comment 42 --body hi', 'glab mr approve 42', 'git push origin main']) {
      const result = invoke(host, 'craft-review', 'read-only', payload(host, guarded, command), guarded);
      assert.equal(decision(host, result), 'deny', `${host}: ${result.stderr}`);
      assert.equal(result.status, host === 'claude' ? 2 : 0);
    }
    assert.equal(decision(host, invoke(host, 'craft-review', 'read-only', payload(host, guarded, 'git status --short'), guarded)), 'allow');
    assert.equal(decision(host, invoke(host, 'craft-review', 'read-only', payload(host, unrestricted, 'gh pr comment 42 --body hi'), unrestricted)), 'allow');
  }
});

// Hosts treat a failing hook (exit 1) as non-blocking, so unread input must deny explicitly.
test('read-only review denies shell input it could not read in full', (t) => {
  const guarded = fixture(t, { review: { readOnly: true } });
  const unrestricted = fixture(t);
  for (const host of native) {
    const command = `gh pr comment 42 --body ${'x'.repeat(70000)}`;
    const result = invoke(host, 'craft-review', 'read-only', payload(host, guarded, command), guarded);
    assert.equal(decision(host, result), 'deny', `${host}: ${result.stderr}`);
    assert.equal(result.status, host === 'claude' ? 2 : 0, `${host}: ${result.stderr}`);
    assert.notEqual(decision(host, invoke(host, 'craft-review', 'read-only', payload(host, unrestricted, command), unrestricted)), 'deny');
  }
});

test('malformed payloads and unrelated tools produce valid nonblocking wire responses', (t) => {
  const dir = fixture(t, { review: { readOnly: true } });
  for (const host of native) for (const input of ['bad json', 'null', '[]', JSON.stringify({ ...payload(host, dir, 'gh pr comment 1'), tool_name: 'Read', toolName: 'view' })]) {
    const result = invoke(host, 'craft-review', 'read-only', input, dir);
    assert.equal(result.status, 0, `${host}: ${result.stderr}`);
    assert.equal(decision(host, result), 'allow');
  }
});

// Craft only denies. Where the host takes no answer as its default, a call Craft lets through
// gets none, so the user's own approvals decide; Cursor requires a permission and treats
// `allow` as advisory.
test('a tool call Craft does not deny keeps the host permission flow', (t) => {
  const dir = fixture(t, { review: { readOnly: true } });
  for (const host of native) for (const [pkg, hook] of [['craft-review', 'read-only'], ['craft-delivery', 'routing'], ['craft-engineering', 'observe-before-tool']]) {
    for (const tool of ['shell', 'edit']) {
      const result = invoke(host, pkg, hook, payload(host, dir, 'git status --short', tool), dir);
      assert.equal(result.status, 0, `${host} ${hook}: ${result.stderr}`);
      // Cursor requires an answer, observation hooks included.
      if (host === 'cursor') assert.deepEqual(JSON.parse(result.stdout), { permission: 'allow' }, `${hook} ${tool}`);
      else {
        const output = result.stdout.trim() ? JSON.parse(result.stdout) : {};
        const granted = [output.permission, output.permissionDecision, output.decision, output.hookSpecificOutput?.permissionDecision];
        assert.ok(!granted.includes('allow'), `${host} ${hook} ${tool}: ${result.stdout}`);
      }
    }
  }
});

test('prompt routing hints appear only where the native context schema supports them', (t) => {
  const dir = fixture(t);
  for (const host of native) {
    const result = invoke(host, 'craft-delivery', 'prompt-routing', { cwd: dir, session_id: 'fixture-session', prompt: 'review this change' }, dir);
    assert.equal(result.status, 0, result.stderr);
    const output = result.stdout.trim() ? JSON.parse(result.stdout) : {};
    const context = output.hookSpecificOutput?.additionalContext ?? output.additionalContext ?? output.additional_context ?? '';
    assert.equal(context.includes('Routing hints'), HOSTS[host].events['before-prompt'].context, host);
    if (context) assert.ok(context.includes(HOSTS[host].listing.names === 'plugin' ? 'craft-review:review-change' : 'craft-review-review-change'), host);
  }
});

// One declared replacement in `.agents/skills`: hosts that read the folder are sent to the
// consumer skill by its bare name; Claude Code does not read it, so keeps the package skill.
test('a declared consumer skill replaces the package skill on every host that can load it', (t) => {
  const dir = fixture(t, { skills: { declared: [{ name: 'team-review', path: '.agents/skills/team-review', replaces: ['craft-review:review-change'] }] } });
  fs.mkdirSync(path.join(dir, '.agents/skills/team-review'), { recursive: true });
  fs.writeFileSync(path.join(dir, '.agents/skills/team-review/SKILL.md'), '---\nname: team-review\ndescription: Our review.\n---\n');
  for (const host of native) {
    if (!HOSTS[host].events['before-prompt'].context) continue;
    const result = invoke(host, 'craft-delivery', 'prompt-routing', { cwd: dir, session_id: `skills-${host}`, prompt: 'review this change' }, dir);
    assert.equal(result.status, 0, result.stderr);
    const output = result.stdout.trim() ? JSON.parse(result.stdout) : {};
    const context = output.hookSpecificOutput?.additionalContext ?? output.additionalContext ?? output.additional_context ?? '';
    const line = context.split('\n').find(text => text.startsWith('Routing hints')) ?? '';
    const hint = JSON.parse(line.slice(line.indexOf('{')));
    const reads = HOSTS[host].listing.projectSkills.includes('.agents/skills');
    assert.equal(hint.skills.includes('team-review'), reads, `${host}: ${context}`);
    assert.equal(hint.skills.some(skill => skill.includes('review-change')), !reads, `${host}: ${context}`);
    assert.equal((hint.missing ?? []).some(line => line.includes('project:team-review unavailable')), !reads, `${host}: ${context}`);
  }
});

test('a lookup prompt without a verified answer gets no hint', (t) => {
  const dir = fixture(t);
  fs.writeFileSync(path.join(dir, 'orders.py'), 'RATE = LEGACY_TAX_RATE\n');
  // A real project: the lookup reads its Git inventory.
  for (const args of [['init', '-q'], ['add', '-A']]) assert.equal(spawnSync('git', args, { cwd: dir }).status, 0);
  for (const host of native) {
    const result = invoke(host, 'craft-delivery', 'prompt-routing', { cwd: dir, session_id: `lookup-${host}`, prompt: 'Where is LEGACY_TAX_RATE used?' }, dir);
    assert.equal(result.status, 0, result.stderr);
    const output = result.stdout.trim() ? JSON.parse(result.stdout) : {};
    const context = output.hookSpecificOutput?.additionalContext ?? output.additionalContext ?? output.additional_context ?? '';
    assert.ok(!context.includes('Routing hints'), `${host}: a pointer to a command costs the model a turn`);
  }
});

test('a callers prompt gets cited rows only when the name has one declaration', (t) => {
  const dir = fixture(t);
  const java = (name, body) => fs.writeFileSync(path.join(dir, `${name}.java`), `class ${name} {\n${body}\n}\n`);
  java('Policy', '  long apply(long cents) { return cents; }');
  java('Checkout', '  long finish(Policy p) {\n    return p.apply(10);\n  }');
  java('Quote', '  long preview(Policy p) {\n    return p.apply(5);\n  }');
  // `total` is declared twice: its callers may call either one.
  java('Cart', '  long total() { return 1; }\n  long sum(Cart c) { return c.total(); }');
  java('Order', '  long total() { return 2; }');
  // `charge` is also called from a script no index answers for: the callers row set is open.
  java('Fee', '  long charge() { return 3; }\n  long twice(Fee f) { return f.charge() * 2; }');
  fs.writeFileSync(path.join(dir, 'bill.kt'), 'fun bill(fee: Fee) = fee.charge()\n');
  for (const args of [['init', '-q'], ['add', '-A']]) assert.equal(spawnSync('git', args, { cwd: dir }).status, 0);
  const hint = (host, prompt) => {
    const result = invoke(host, 'craft-delivery', 'prompt-routing', { cwd: dir, session_id: `answer-${host}-${prompt.length}`, prompt }, dir);
    assert.equal(result.status, 0, result.stderr);
    const output = result.stdout.trim() ? JSON.parse(result.stdout) : {};
    const context = output.hookSpecificOutput?.additionalContext ?? output.additionalContext ?? output.additional_context ?? '';
    return context && JSON.parse(context.slice(context.indexOf('{')));
  };
  for (const host of native.filter(host => HOSTS[host].events['before-prompt'].context)) {
    const answered = hint(host, 'Which methods call `Policy.apply`?');
    assert.equal(answered.intent, 'locate', host);
    assert.equal(answered.skills, undefined, `${host}: a ready lookup names no skill`);
    assert.deepEqual(answered.answer.rows, ['Checkout.java:3 in finish: return p.apply(10);', 'Quote.java:3 in preview: return p.apply(5);'], host);
    assert.equal(answered.answer.complete, true);
    assert.equal(answered.answer.basis, 'name', `${host}: callers match the called name only`);
    // The answered hint's lookup runs as given, for a search beyond the answered question.
    const [program, ...args] = answered.lookup.run;
    const lookup = spawnSync(program, [...args, '--callers', 'apply'], { cwd: dir, encoding: 'utf8' });
    assert.equal(lookup.status, 0, `${host}: ${lookup.stderr}`);
    assert.match(lookup.stdout, /Checkout\.java/, host);
    assert.equal(hint(host, 'Who calls `total`?'), '', `${host}: an ambiguous name gets no hint`);
    assert.equal(hint(host, 'Where is LEGACY_TAX_RATE used?'), '', host);
    assert.equal(hint(host, 'Who calls `Fee.charge`?'), '', `${host}: a literal match outside the index leaves callers open`);
  }
});

test('a search the model ran gets the verified declaration and callers in the same result', (t) => {
  const dir = fixture(t);
  const java = (name, body) => fs.writeFileSync(path.join(dir, `${name}.java`), `class ${name} {\n${body}\n}\n`);
  java('Policy', '  long apply(long cents) { return cents; }');
  java('Checkout', '  long finish(Policy p) {\n    return p.apply(10);\n  }');
  java('Quote', '  long preview(Policy p) {\n    return p.apply(5);\n  }');
  for (const args of [['init', '-q'], ['add', '-A']]) assert.equal(spawnSync('git', args, { cwd: dir }).status, 0);
  const shell = { codex: 'Bash', cursor: 'Shell', gemini: 'run_shell_command' };
  const context = (host, input) => {
    const result = invoke(host, 'craft-delivery', 'milestone', { session_id: `search-${host}`, cwd: dir, ...input }, dir);
    assert.equal(result.status, 0, result.stderr);
    const output = result.stdout.trim() ? JSON.parse(result.stdout) : {};
    return output.hookSpecificOutput?.additionalContext ?? output.additionalContext ?? output.additional_context ?? '';
  };
  const answer = text => JSON.parse(text.slice(text.indexOf('{')));
  // A search answer may only refresh what changed since the index was warmed; warm it the way
  // a session does, so a slow runner does not spend the search's budget on a cold index.
  const warm = invoke('claude', 'craft-delivery', 'prompt-routing', { cwd: dir, session_id: 'search-warm', prompt: 'Which methods call `Policy.apply`?' }, dir);
  assert.equal(warm.status, 0, warm.stderr);
  const grep = context('claude',{ tool_name: 'Grep', tool_input: { pattern: 'apply' } });
  assert.match(grep, /^Project Intelligence for the search just run/);
  assert.match(answer(grep).declaration, /^Policy\.java:2/);
  assert.deepEqual(answer(grep).rows, ['Checkout.java:3 in finish: return p.apply(10);', 'Quote.java:3 in preview: return p.apply(5);']);
  for (const [host, tool] of Object.entries(shell)) {
    const text = context(host, { tool_name: tool, tool_input: { command: `cd ${JSON.stringify(dir)} && rg -n apply | head` } });
    if (!HOSTS[host].events['after-tool'].context) { assert.equal(text, '', host); continue; }
    assert.deepEqual(answer(text).rows, answer(grep).rows, host);
  }
  assert.equal(context('claude', { tool_name: 'Grep', tool_input: { pattern: 'app.*' } }), '', 'a regex names no symbol');
  assert.equal(context('claude', { tool_name: 'Grep', tool_input: { pattern: 'finish' } }), '', 'a name nothing calls gets no answer');
  const copilot = invoke('copilot', 'craft-delivery', 'milestone', { sessionId: 'search-copilot', cwd: dir, toolName: 'bash', toolArgs: { command: 'rg -n apply' } }, dir);
  assert.equal(copilot.status, 0, copilot.stderr);
  assert.deepEqual(answer(JSON.parse(copilot.stdout).additionalContext).rows, answer(grep).rows, 'copilot appends the answer to the tool result');
  fs.writeFileSync(path.join(dir, '.craft/project.json'), JSON.stringify({ formatVersion: 1, efficiency: { lookup: { enabled: false } } }));
  assert.equal(context('claude', { tool_name: 'Grep', tool_input: { pattern: 'apply' } }), '', 'lookup disabled: no search answers');
});

test('a key, mention or decision-record prompt gets its verified answer on any route, and the switches turn hints and answers off', (t) => {
  const dir = fixture(t);
  fs.writeFileSync(path.join(dir, 'tsconfig.json'), '{\n  // compiler\n  "compilerOptions": {\n    "strict": true,\n  },\n}\n');
  fs.mkdirSync(path.join(dir, 'docs/adr'), { recursive: true });
  fs.writeFileSync(path.join(dir, 'docs/adr/0001-ids.md'), '# Use numbers as IDs\n\n- Status: superseded by [0002](0002-slugs.md)\n');
  fs.writeFileSync(path.join(dir, 'docs/adr/0002-slugs.md'), '# Use slugs as IDs\n\nStatus: accepted\n\nReplaces `AdrNumber`; see [code](../../src/slug.ts).\n');
  for (const args of [['init', '-q'], ['add', '-A']]) assert.equal(spawnSync('git', args, { cwd: dir }).status, 0);
  const hint = (host, prompt, session = `noncode-${host}-${prompt.length}`) => {
    const result = invoke(host, 'craft-delivery', 'prompt-routing', { cwd: dir, session_id: session, prompt }, dir);
    assert.equal(result.status, 0, result.stderr);
    const output = result.stdout.trim() ? JSON.parse(result.stdout) : {};
    const context = output.hookSpecificOutput?.additionalContext ?? output.additionalContext ?? output.additional_context ?? '';
    return context && JSON.parse(context.slice(context.indexOf('{')));
  };
  const capable = native.filter(host => HOSTS[host].events['before-prompt'].context);
  for (const host of capable) {
    const key = hint(host, 'Where is `compilerOptions.strict` set?').answer;
    assert.deepEqual([key.rows, key.basis], [['tsconfig.json:4 in compilerOptions: compilerOptions.strict: "strict": true,'], 'parsed'], host);
    assert.deepEqual(hint(host, 'Which docs mention `AdrNumber`?').answer.rows, ['docs/adr/0002-slugs.md:5 in Use slugs as IDs: Replaces `AdrNumber`; see [code](../../src/slug.ts).'], host);
    assert.deepEqual(hint(host, 'Which documents link to src/slug.ts?').answer.rows.length, 1, host);
    assert.deepEqual(hint(host, 'Which ADR is the current decision on IDs?').answer.rows, [
      'docs/adr/0001-ids.md:1: Use numbers as IDs [superseded by docs/adr/0002-slugs.md]',
      'docs/adr/0002-slugs.md:1: Use slugs as IDs [accepted]',
    ], host);
  }
  fs.writeFileSync(path.join(dir, '.craft/project.json'), JSON.stringify({ formatVersion: 1, routing: { promptHints: false } }));
  for (const host of capable) {
    assert.equal(hint(host, 'review this change', `quiet-${host}`), '', `${host}: prompt hints off`);
    assert.ok(hint(host, 'Where is `compilerOptions.strict` set?', `quiet-answer-${host}`).answer, `${host}: answers follow their own switch`);
  }
  fs.writeFileSync(path.join(dir, '.craft/project.json'), JSON.stringify({ formatVersion: 1, efficiency: { lookup: { enabled: false } } }));
  for (const host of capable) assert.equal(hint(host, 'Where is `compilerOptions.strict` set?', `off-${host}`), '', `${host}: lookup disabled: no prompt answers`);
});

test('explicit verify prompts select testing-only context rather than feature implementation', (t) => {
  const dir = fixture(t);
  for (const host of native) {
    const result = invoke(host, 'craft-delivery', 'prompt-routing', { cwd: dir, prompt: 'verify this feature' }, dir);
    assert.equal(result.status, 0, result.stderr);
    const capable = HOSTS[host].events['before-prompt'].context;
    assert.equal(result.stdout.includes('workflow-verify-feature'), capable, host);
    assert.equal(result.stdout.includes('testing-only'), capable, host);
    assert.ok(!/apply-change|deliver-change/.test(result.stdout), host);
  }
});

test('edit routing defaults on and can be disabled and prompt hints omit the full catalog evidence', (t) => {
  const quiet = fixture(t, { routing: { editHints: false } });
  for (const host of native) {
    // One folder per host: an edit routes once per file in a session.
    const enabled = fixture(t);
    const ordinary = invoke(host, 'craft-delivery', 'routing', payload(host, quiet, '', 'edit'), quiet);
    assert.equal(ordinary.status, 0, ordinary.stderr);
    assert.ok(!ordinary.stdout.includes('Routing hints'), host);
    const opted = invoke(host, 'craft-delivery', 'routing', payload(host, enabled, '', 'edit'), enabled);
    assert.equal(opted.status, 0, opted.stderr);
    assert.equal(opted.stdout.includes('Routing hints'), HOSTS[host].events['before-tool'].context, host);
    assert.ok(!opted.stdout.includes('documentationQuestions'), 'full route remains available through the CLI');
    assert.ok(!opted.stdout.includes('selections'), 'catalog evidence must not be injected on every edit');
  }
});

test('opted-in native hooks reuse only emitted hints and re-emit after compaction', (t) => {
  for (const host of native) {
    const dir = fixture(t, { routing: { dedupeHints: true } });
    const input = { cwd: dir, session_id: 'hint-session', sessionId: 'hint-session', prompt: 'review the scoped change' };
    const first = invoke(host, 'craft-delivery', 'prompt-routing', input, dir);
    const next = invoke(host, 'craft-delivery', 'prompt-routing', input, dir);
    assert.equal(first.status, 0, first.stderr);
    assert.equal(next.status, 0, next.stderr);
    const capable = HOSTS[host].events['before-prompt'].context;
    assert.equal(first.stdout.includes('Routing hints'), capable, host);
    assert.ok(!next.stdout.includes('Routing hints'), host);
    // A host that cannot show the hint still records the intent the request established.
    if (!capable) assert.ok(only(stateNames(dir), /^(?:manifest\.json|intent\.json)/), host);
    else assert.ok(Buffer.byteLength(next.stdout) < Buffer.byteLength(first.stdout));
    const compact = invoke(host, 'craft-delivery', 'checkpoint', { cwd: dir, session_id: 'hint-session', sessionId: 'hint-session' }, dir);
    assert.equal(compact.status, 0, compact.stderr);
    const resumed = invoke(host, 'craft-delivery', 'prompt-routing', input, dir);
    assert.equal(resumed.stdout.includes('Routing hints'), capable, host);
  }
});

test('a hint that reads the same is not sent again for another file, and empty fields are omitted', (t) => {
  for (const host of native) {
    const dir = fixture(t);
    const edit = file => invoke(host, 'craft-delivery', 'routing', { ...payload(host, dir, '', 'edit'), ...(host === 'copilot' ? { toolArgs: { path: file } } : { tool_input: { file_path: file } }) }, dir);
    const first = edit('src/total.mjs'), second = edit('src/total.test.mjs'), other = edit('scripts/build.py');
    for (const result of [first, second, other]) assert.equal(result.status, 0, result.stderr);
    const capable = HOSTS[host].events['before-tool'].context;
    assert.equal(first.stdout.includes('Routing hints'), capable, host);
    assert.ok(!second.stdout.includes('Routing hints'), `${host}: identical hint re-sent`);
    // A different language changes what the host is shown, so it routes again.
    assert.equal(other.stdout.includes('python.md'), capable, host);
    if (capable) for (const empty of ['"modules":[]', '"missing":[]', '"task":null']) assert.ok(!first.stdout.includes(empty.replaceAll('"', '\\"')), `${host}: ${empty}`);
  }
});

test('session service preparation reads metadata without probing executable availability', (t) => {
  const dir = fixture(t, { services: { git: { provider: 'git', transport: 'cli', command: 'never-execute-this-command' } } });
  for (const host of native) {
    const result = invoke(host, 'craft-delivery', 'restore', { cwd: dir, session_id: 'fixture-session' }, dir);
    assert.equal(result.status, 0, result.stderr);
    assert.ok(result.stdout.includes('never-execute-this-command'), host);
    assert.ok(result.stdout.includes('unverified'), host);
    assert.equal(readCheckpoint(dir, 'fixture-session'), null, 'metadata preparation creates no checkpoint');
    assert.ok(only(stateNames(dir), /^(?:manifest\.json|routing-)/), 'only telemetry and routing invalidation state are allowed');
  }
});

test('compaction observers save checkpoints without claiming to control compression', (t) => {
  const dir = fixture(t, { checkpoint: { enabled: true } });
  saveCheckpoint(dir, 'fixture-session', { task: 'Fixture task', authorization: 'local review', nextStep: 'inspect snapshot' });
  for (const host of native) {
    const result = invoke(host, 'craft-delivery', 'checkpoint', { cwd: dir, session_id: 'fixture-session', trigger: 'auto' }, dir);
    assert.equal(result.status, 0, `${host}: ${result.stderr}`);
    assert.equal(readCheckpoint(dir, 'fixture-session').task, 'Fixture task');
    const output = result.stdout.trim() ? JSON.parse(result.stdout) : {};
    assert.ok(output.continue !== false && output.decision !== 'block');
  }
});

test('checkpoint state can be explicitly disabled', (t) => {
  const dir = fixture(t, { checkpoint: { enabled: false } });
  for (const host of native) {
    const result = invoke(host, 'craft-delivery', 'checkpoint', { cwd: dir, session_id: 'fixture-session' }, dir);
    assert.equal(result.status, 0, result.stderr);
  }
  assert.equal(readCheckpoint(dir, 'fixture-session'), null);
  // Context lifecycle observations remain independent of checkpoint writes.
  assert.ok(only(stateNames(dir), /^(?:manifest\.json|routing-|host-context\.json)/));
});

test('unsupported lifecycle mappings are reported and no fake wrapper is installed', () => {
  for (const host of ['cursor', 'copilot', 'gemini', 'agents']) {
    const output = path.join(root, 'dist', host);
    const report = JSON.parse(fs.readFileSync(path.join(output, 'adapter-report.json'), 'utf8'));
    const delivery = report.packages.find((entry) => entry.package === 'craft-delivery');
    assert.equal(delivery.hookMappings.find((entry) => entry.id === 'compact-restore').supported, false);
    const registered = JSON.stringify(host === 'agents' ? {} : JSON.parse(fs.readFileSync(path.join(output, { cursor: '.cursor/hooks.json', copilot: '.github/hooks/craft-capabilities.json', gemini: '.gemini/settings.json' }[host]), 'utf8')));
    assert.ok(!registered.includes('after-compact'), `${host} registers no unsupported hook`);
  }
});


test('Claude and Codex restore context through the documented compact session-start event', (t) => {
  const dir = fixture(t, { checkpoint: { enabled: true } });
  saveCheckpoint(dir, 'fixture-session', { task: 'Resume only the scoped review', authorization: 'read-only', nextStep: 'inspect retry code' });
  for (const host of ['claude', 'codex']) {
    const result = invoke(host, 'craft-delivery', 'compact-restore', { cwd: dir, source: 'compact', session_id: 'fixture-session' }, dir);
    assert.equal(result.status, 0, result.stderr);
    const output = JSON.parse(result.stdout).hookSpecificOutput;
    assert.equal(output.hookEventName, 'SessionStart');
    assert.match(output.additionalContext, /Resume only the scoped review/);
    assert.equal(HOSTS[host].events['after-compact'].matcher, 'compact');
  }
});

test('Copilot runs the same runtime arguments from Bash and PowerShell', () => {
  const config = JSON.parse(fs.readFileSync(path.join(root, 'dist/copilot/.github/hooks/craft-capabilities.json'), 'utf8'));
  for (const hooks of Object.values(config.hooks)) for (const hook of hooks) {
    const bash = registeredHook(hook.bash);
    assert.match(bash.root, /^\.\/packages\/[a-z-]+$/);
    assert.match(bash.args, /^hook copilot /);
    assert.deepEqual(registeredHook(hook.powershell), bash);
    assert.ok(!/\bnode\b/.test(hook.bash + hook.powershell), 'hooks need no Node.js');
  }
});

test('PowerShell shell tools cannot bypass read-only review normalization', (t) => {
  const dir = fixture(t, { review: { readOnly: true } });
  for (const name of ['powershell', 'pwsh']) {
    const result = invoke('copilot', 'craft-review', 'read-only', payload('copilot', dir, 'git push origin main', name), dir);
    // payload() normally maps non-shell tool names to edit; exercise native toolName explicitly.
    const native = invoke('copilot', 'craft-review', 'read-only', { cwd: dir, toolName: name, toolArgs: { command: 'git push origin main' } }, dir);
    assert.equal(native.status, 0, native.stderr);
    assert.equal(decision('copilot', native), 'deny');
    assert.equal(decision('copilot', result), 'allow', 'unrelated edit payload stays out of shell policy');
  }
});

test('the project schema accepts exactly the skill folders some host reads', () => {
  const schema = JSON.parse(fs.readFileSync(path.join(root, 'schemas/project.schema.json'), 'utf8'));
  const pattern = new RegExp(schema.properties.skills.properties.declared.items.properties.path.pattern);
  const folders = new Set(Object.values(HOSTS).flatMap(host => host.listing.projectSkills));
  for (const folder of folders) assert.ok(pattern.test(`${folder}/x`), folder);
  const named = /\((.*)\)/.exec(pattern.source)[1].split('|').map(name => `.${name}/skills`);
  assert.deepEqual(named.sort(), [...folders].sort());
});
