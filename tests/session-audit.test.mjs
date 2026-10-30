import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import test from 'node:test';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { deliverHook } from './native-hook.mjs';
import { audit, markdown, readTranscript, sessionEvents } from '../tooling/session-audit.mjs';

const script = fileURLToPath(new URL('../tooling/session-audit.mjs', import.meta.url));

function project(t) {
  const root = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-session-audit-')));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.writeFileSync(path.join(root, 'index.js'), 'export const x = 1;\n');
  assert.equal(spawnSync('git', ['-C', root, 'init', '-q']).status, 0);
  return root;
}
const write = (root, name, records) => {
  const file = path.join(root, name);
  fs.writeFileSync(file, `${records.map(record => JSON.stringify(record)).join('\n')}\nnot json\n`);
  return file;
};
const usage = { input_tokens: 10, cache_creation_input_tokens: 5, cache_read_input_tokens: 100, output_tokens: 7 };
const assistant = (root, session, id, calls) => calls.map(input => ({ type: 'assistant', sessionId: session, cwd: root, timestamp: '2026-10-30T10:00:01Z',
  message: { id, usage, content: [{ type: 'tool_use', ...input }] } }));

test('a Claude session joins host tool use with the events Craft recorded for that session only', t => {
  const root = project(t), session = 'audited-session';
  // Real hook deliveries record the session pseudonym exactly as an installed host does.
  deliverHook(root, 'before-prompt', { prompt: 'Can you plan the next step?', session, host: 'claude' });
  deliverHook(root, 'before-tool', { tool: 'read', file: path.join(root, 'index.js'), session, host: 'claude' });
  deliverHook(root, 'before-prompt', { prompt: 'unrelated', session: 'other-session', host: 'claude' });
  const transcript = write(root, `${session}.jsonl`, [
    { type: 'user', sessionId: session, cwd: root, timestamp: '2026-10-30T10:00:00Z', message: { content: 'Can you plan the next step?' } },
    // Two blocks of one model call repeat its usage; a second call adds once more.
    ...assistant(root, session, 'm1', [{ name: 'Read', input: { file_path: 'index.js' } }, { name: 'Read', input: { file_path: 'index.js' } }]),
    ...assistant(root, session, 'm2', [{ name: 'Bash', input: { command: 'rg -n x . && craft locate --root . x' } },
      { name: 'Skill', input: { skill: 'craft-delivery:apply-change' } }]),
    { type: 'user', sessionId: session, timestamp: '2026-10-30T10:00:02Z', message: { content: [{ type: 'tool_result', content: 'ok' }] } },
  ]);
  const read = readTranscript(transcript);
  assert.deepEqual([read.host, read.session, read.root, read.prompts, read.modelCalls], ['claude', session, root, 1, 2]);
  assert.deepEqual(read.tokens, { input: 30, cached: 200, output: 14 });

  const events = sessionEvents(root, session);
  assert.ok(events.length > 0);
  const card = audit(read, events, null);
  assert.equal(card.craft.hooks.count, 2, 'the other session’s hook is excluded');
  assert.deepEqual([card.agent.toolCalls, card.agent.fileReads, card.agent.rereads, card.agent.searchCommands], [4, 2, 1, 1]);
  assert.deepEqual(card.agent.skillsInvoked, ['craft-delivery:apply-change']);
  assert.deepEqual(card.agent.craftCommands, { locate: 1 });
  assert.ok(card.signals.some(signal => signal.startsWith('no-routing')));
  assert.equal(card.signals.some(signal => signal.startsWith('craft-silent')), false);
  assert.match(markdown(card), /\| Hooks \| 2 \(p50 \d+ ms/);

  // The command line adds Craft's own report for the same session.
  const child = spawnSync(process.execPath, [script, transcript, '--json'], { encoding: 'utf8' });
  assert.equal(child.status, 0, child.stderr);
  const cli = JSON.parse(child.stdout);
  assert.equal(cli.craft.features.nativeHooks.events, 2);
  assert.equal(cli.craft.events, card.craft.events, 'the report records its own command outside the session');
});

test('a Codex rollout counts turns and commands, keeps absent values unknown and flags a silent Craft', t => {
  const root = project(t);
  const transcript = write(root, 'rollout.jsonl', [
    { type: 'session_meta', timestamp: '2026-10-30T09:00:00Z', payload: { id: 'codex-thread', cwd: root } },
    { type: 'response_item', payload: { type: 'message', role: 'user', content: [{ type: 'input_text', text: '# AGENTS.md instructions' }] } },
    { type: 'event_msg', payload: { type: 'task_started' } },
    { type: 'response_item', payload: { type: 'function_call', name: 'exec_command', arguments: JSON.stringify({ cmd: 'grep -rn total src' }) } },
    { type: 'response_item', payload: { type: 'custom_tool_call', name: 'exec', input: 'text(await tools.exec_command({cmd:"rg -n \\"a b\\" src",max_output_tokens:3}));' } },
    { type: 'event_msg', payload: { type: 'token_count', info: { total_token_usage: { input_tokens: 500, cached_input_tokens: 400, output_tokens: 20 } } } },
  ]);
  const read = readTranscript(transcript);
  assert.deepEqual([read.host, read.session, read.prompts], ['codex', 'codex-thread', 1]);
  assert.deepEqual(read.commands, ['grep -rn total src', 'rg -n "a b" src']);
  const card = audit(read, sessionEvents(root, read.session), null);
  assert.deepEqual(read.tokens, { input: 100, cached: 400, output: 20 });
  assert.deepEqual([card.agent.modelCalls, card.agent.fileReads, card.agent.skillsInvoked, card.craft.hooks.maxMs], [null, null, null, null]);
  assert.deepEqual(card.signals, ['craft-silent: no Craft event is attributed to this session']);
  assert.match(markdown(card), /\| Model calls \| unknown \|/);
});

test('an unknown transcript is rejected', t => {
  const root = project(t);
  assert.throws(() => readTranscript(write(root, 'x.jsonl', [{ hello: 1 }])), /unrecognized transcript/);
});
