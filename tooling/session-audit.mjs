#!/usr/bin/env node
// One real session's engagement card: what Craft recorded for it, joined with what the host
// transcript shows the agent did. Read-only; it does not call a model or change Craft state.
// Transcript formats belong to each host and change between versions, so transcript counts
// are best-effort and a value the transcript does not carry stays unknown (null).
import fs from 'node:fs';
import path from 'node:path';
import { createHmac } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { nativeBinary } from './lib/native-binary.mjs';

const SEARCH = /(?:^|[\s;&|(])(?:rg|grep|egrep|find|fd|ag|ack)\s|git\s+grep\s/;
const CRAFT = /(?:^|[\s;&|(/])craft(?:\.exe)?\s+([a-z][a-z-]*)|npm run (locate|route|session|context:report|telemetry:report)\b/g;
const LOOKUPS = new Set(['locate', 'question', 'route']);
// Thresholds for the signals; they flag a session for a closer look, they do not grade it.
const MANY_SEARCHES = 5, MANY_REREADS = 3, SLOW_HOOK_MS = 1000, START_SLACK_MS = 60000;

const lines = file => fs.readFileSync(file, 'utf8').split('\n').flatMap(line => {
  try { return line.trim() ? [JSON.parse(line)] : []; } catch { return []; }
});
const increment = (counts, key, by = 1) => { counts[key] = (counts[key] ?? 0) + by; return counts; };

function claude(records) {
  const session = records.find(r => r.sessionId)?.sessionId, root = records.find(r => r.cwd)?.cwd;
  const tools = {}, commands = [], reads = [], skills = [], usage = new Map();
  let prompts = 0;
  for (const record of records) {
    const content = Array.isArray(record.message?.content) ? record.message.content : [];
    if (record.type === 'user' && !record.isMeta && (typeof record.message?.content === 'string' || content.some(c => c.type === 'text'))) prompts++;
    if (record.type !== 'assistant') continue;
    // Each content block repeats its message's usage; count every model call once.
    if (record.message?.id && record.message.usage) usage.set(record.message.id, record.message.usage);
    for (const call of content.filter(c => c.type === 'tool_use')) {
      increment(tools, call.name);
      if (call.name === 'Bash' && call.input?.command) commands.push(call.input.command);
      if (call.name === 'Read' && call.input?.file_path) reads.push(call.input.file_path);
      if (call.name === 'Skill' && call.input?.skill) skills.push(call.input.skill);
    }
  }
  const sum = key => [...usage.values()].reduce((total, u) => total + (u[key] ?? 0), 0);
  return { host: 'claude', session, root, prompts, tools, commands, reads, skills, modelCalls: usage.size,
    tokens: { input: sum('input_tokens') + sum('cache_creation_input_tokens'), cached: sum('cache_read_input_tokens'), output: sum('output_tokens') },
    window: window(records.map(r => r.timestamp)) };
}

function codex(records) {
  const meta = records.find(r => r.type === 'session_meta')?.payload ?? {};
  const tools = {}, commands = [];
  let prompts = 0, total = null;
  for (const { type, payload = {} } of records) {
    // One turn starts per user prompt; injected instructions arrive as user messages too.
    if (type === 'event_msg' && payload.type === 'task_started') prompts++;
    if (type === 'event_msg' && payload.type === 'token_count' && payload.info?.total_token_usage) total = payload.info.total_token_usage;
    if (type !== 'response_item') continue;
    if (payload.type === 'function_call') {
      increment(tools, payload.name);
      try {
        const { cmd, command } = JSON.parse(payload.arguments);
        const text = cmd ?? command;
        if (text) commands.push(Array.isArray(text) ? text.join(' ') : text);
      } catch {}
    }
    if (payload.type === 'custom_tool_call') {
      increment(tools, payload.name);
      // Code-mode calls embed shell commands as `cmd:"..."` string literals.
      for (const [, literal] of String(payload.input).matchAll(/cmd:\s*("(?:[^"\\]|\\.)*")/g)) {
        try { commands.push(JSON.parse(literal)); } catch {}
      }
    }
  }
  return { host: 'codex', session: meta.id, root: meta.cwd, prompts, tools, commands, reads: null, skills: null, modelCalls: null,
    tokens: total && { input: total.input_tokens - (total.cached_input_tokens ?? 0), cached: total.cached_input_tokens ?? null, output: total.output_tokens },
    window: window(records.map(r => r.timestamp)) };
}

function window(stamps) {
  const sorted = stamps.filter(Boolean).sort();
  return { first: sorted[0] ?? null, last: sorted.at(-1) ?? null };
}

export function readTranscript(file) {
  const records = lines(file);
  if (records.some(r => r.type === 'session_meta')) return codex(records);
  if (records.some(r => r.type === 'assistant' || r.type === 'user')) return claude(records);
  throw new Error('unrecognized transcript: expected a Claude Code session or a Codex rollout');
}

// The session's own raw events: the retained log filtered by the session pseudonym that
// `craft` records (HMAC-SHA-256 of the host session under the project key, 32 hex digits).
export function sessionEvents(root, session) {
  const directory = path.join(root, '.craft', 'state', 'telemetry');
  let key;
  try { ({ key } = JSON.parse(fs.readFileSync(path.join(directory, 'key.json'), 'utf8'))); } catch { return []; }
  const id = createHmac('sha256', key).update(session).digest('hex').slice(0, 32);
  return ['events.jsonl.1', 'events.jsonl'].flatMap(name => fs.existsSync(path.join(directory, name)) ? lines(path.join(directory, name)) : [])
    .filter(event => event.schemaVersion === 1 && event.sessionId === id);
}

const percentile = (sorted, p) => sorted.length ? sorted[Math.min(sorted.length - 1, Math.ceil(p * sorted.length) - 1)] : null;

export function audit(transcript, events, report) {
  const names = {}, diagnostics = {}, guidance = {};
  for (const event of events) {
    increment(names, event.name);
    if (event.outcome === 'failure') increment(diagnostics, `${event.name}:${event.reasonCode ?? 'unknown'}`);
    if (event.name === 'guidance.selected' && event.attributes?.skill) increment(guidance, event.attributes.skill);
  }
  const hookMs = events.filter(e => e.name === 'host.hook' && Number.isFinite(e.durationMs)).map(e => e.durationMs).sort((a, b) => a - b);
  const craftCommands = {};
  for (const command of transcript.commands) for (const [, sub, script] of command.matchAll(CRAFT)) increment(craftCommands, sub ?? script);
  const searches = transcript.commands.filter(command => SEARCH.test(command)).length;
  const rereads = transcript.reads && transcript.reads.length - new Set(transcript.reads).size;
  const lookups = (names['discovery.completed'] ?? 0) + Object.entries(craftCommands).filter(([name]) => LOOKUPS.has(name)).reduce((t, [, n]) => t + n, 0);
  const card = {
    host: transcript.host, window: transcript.window, prompts: transcript.prompts,
    agent: { modelCalls: transcript.modelCalls, tokens: transcript.tokens ?? null, toolCalls: Object.values(transcript.tools).reduce((t, n) => t + n, 0),
      tools: transcript.tools, searchCommands: searches, fileReads: transcript.reads?.length ?? null, rereads, skillsInvoked: transcript.skills, craftCommands },
    craft: { events: events.length, eventNames: names, hooks: { count: hookMs.length, p50Ms: percentile(hookMs, 0.5), p95Ms: percentile(hookMs, 0.95), maxMs: hookMs.at(-1) ?? null },
      guidanceSelected: guidance, failures: diagnostics, features: report?.features ?? null, completeHistory: report?.storage?.completeHistory ?? null,
      retainedFrom: report?.window?.first ?? null },
  };
  card.signals = signals(card);
  return card;
}

function signals(card) {
  const found = [], count = name => card.craft.eventNames[name] ?? 0;
  if (card.craft.events === 0) found.push('craft-silent: no Craft event is attributed to this session');
  if (card.prompts > 0 && count('host.hook') > 0 && count('routing.completed') === 0) found.push('no-routing: prompts arrived but no work was routed');
  if (card.craft.events > 0 && count('guidance.selected') === 0) found.push('no-guidance: no skill was selected for the agent');
  for (const [failure, n] of Object.entries(card.craft.failures)) {
    if (card.prompts > 0 && n >= card.prompts) found.push(`repeated-failure: ${failure} ×${n}, at least once per prompt`);
  }
  if (card.agent.searchCommands >= MANY_SEARCHES && (count('discovery.completed') + Object.keys(card.agent.craftCommands).filter(n => LOOKUPS.has(n)).length) === 0) {
    found.push(`search-without-lookup: ${card.agent.searchCommands} shell searches, no Craft lookup`);
  }
  if ((card.agent.rereads ?? 0) >= MANY_REREADS) found.push(`rereads: ${card.agent.rereads} repeated file reads`);
  if ((card.craft.hooks.maxMs ?? 0) > SLOW_HOOK_MS) found.push(`slow-hook: slowest hook ${card.craft.hooks.maxMs} ms`);
  if (card.craft.completeHistory === false && card.craft.retainedFrom && card.window.first && Date.parse(card.craft.retainedFrom) - Date.parse(card.window.first) > START_SLACK_MS) {
    found.push(`history-truncated: Craft events before ${card.craft.retainedFrom} were rotated out of the local log`);
  }
  return found;
}

const show = value => value === null || value === undefined ? 'unknown'
  : typeof value === 'object' ? (Object.entries(value).map(([k, v]) => `${k} ${v}`).join(', ') || 'none') : String(value);

export function markdown(card) {
  const rows = [
    ['Host, prompts', `${card.host}, ${card.prompts}`], ['Window', `${show(card.window.first)} – ${show(card.window.last)}`],
    ['Model calls', show(card.agent.modelCalls)], ['Tokens', show(card.agent.tokens)],
    ['Tool calls', `${card.agent.toolCalls} (${show(card.agent.tools)})`], ['Shell searches', show(card.agent.searchCommands)],
    ['File reads, rereads', `${show(card.agent.fileReads)}, ${show(card.agent.rereads)}`],
    ['Skills invoked by the agent', card.agent.skillsInvoked ? (card.agent.skillsInvoked.join(', ') || 'none') : 'unknown'],
    ['Craft commands run', show(card.agent.craftCommands)], ['Craft events', show(card.craft.events)],
    ['Hooks', `${card.craft.hooks.count} (p50 ${show(card.craft.hooks.p50Ms)} ms, p95 ${show(card.craft.hooks.p95Ms)} ms, max ${show(card.craft.hooks.maxMs)} ms)`],
    ['Routing, lookups, checkpoints', ['routing.completed', 'discovery.completed', 'checkpoint.saved'].map(n => card.craft.eventNames[n] ?? 0).join(', ')],
    ['Guidance selected', show(card.craft.guidanceSelected)], ['Failures', show(card.craft.failures)],
    ['Complete Craft history', show(card.craft.completeHistory)],
  ];
  return [`| Measure | Value |`, `|---|---|`, ...rows.map(([k, v]) => `| ${k} | ${v} |`), '',
    card.signals.length ? card.signals.map(s => `- ${s}`).join('\n') : 'No signals.', ''].join('\n');
}

function craftReport(root, session) {
  const child = spawnSync(nativeBinary(), ['telemetry-report', '--root', root, '--session', session], { encoding: 'utf8', shell: false, windowsHide: true });
  try { return child.status === 0 ? JSON.parse(child.stdout) : null; } catch { return null; }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  try {
    const args = process.argv.slice(2), json = args.includes('--json');
    const at = args.indexOf('--root'), root = at >= 0 ? args[at + 1] : undefined;
    const files = args.filter((arg, i) => !arg.startsWith('--') && args[i - 1] !== '--root');
    if (files.length !== 1) throw new Error('usage: npm run eval:audit -- <host-transcript.jsonl> [--root <project>] [--json]');
    const transcript = readTranscript(path.resolve(files[0]));
    const project = path.resolve(root ?? transcript.root ?? '.');
    if (!transcript.session) throw new Error('the transcript names no session');
    const card = audit(transcript, sessionEvents(project, transcript.session), craftReport(project, transcript.session));
    process.stdout.write(json ? `${JSON.stringify(card, null, 2)}\n` : markdown(card));
  } catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 2; }
}
