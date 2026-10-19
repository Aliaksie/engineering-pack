#!/usr/bin/env node
import { flushTelemetry, withTelemetry, observe } from '../application/telemetry/observe.mjs';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { execute } from './requests.mjs';
import { renderSessionStatus } from './render.mjs';

const help = {
  session: { usage: 'session [--root project] [--session id --task task-id] [--data task.json] [--json]',
    taskFields: ['id', 'revision', 'stage', 'operation', 'status', 'files', 'safeBoundary', 'phaseChanged', 'almostComplete', 'compacted', 'staleAssumptions', 'risks'], mode: 'recommendation-only' },
  locate: { usage: 'locate --root project --term Foo --term Bar [--path src] [--session id --task task-id] [--max-output-bytes 32768]',
    data: 'Alternatively --data request.json: terms or independent requests {id,term,dependsOn}, paths, maxFiles, maxMatches, maxBytes, maxOutputBytes', mode: 'bounded literal scan; optional local reuse receipts' },
  'session-attempt': { usage: 'session-attempt --root project --session id --task task-id --data attempt.json',
    example: { operation: 'implement', target: 'acceptance:regression', approach: 'bounded-fix', evidence: 'artifacts/check.json', outcome: 'passed' },
    outcomes: ['passed', 'quality-failure', 'permission-denied', 'tool-unavailable', 'blocked'], optional: ['profile', 'maxAttempts', 'escalationReason'], mode: 'receipt only; no retry execution' },
  'checkpoint-read': { usage: 'checkpoint-read --root project --session id', mode: 'read-only recovery comparison' },
  'checkpoint-save': { usage: 'checkpoint-save --root project --session id [--task task-id] --data handoff.json',
    continuationFields: ['goal', 'currentState', 'completed', 'approach', 'evidence', 'filesChanged', 'validation', 'remainingWork', 'blockers', 'nextStep'], mode: 'explicit local checkpoint write' }
};

export function run(argv) {
  const [action, ...args] = argv;
  if (action === 'help' || action === '--help' || args.includes('--help')) return help[action] ?? { commands: ['route', 'context-report', 'session', 'session-advice', 'session-attempt', 'model-advice', 'lookup', 'locate', 'prepare-services', 'operation', 'checkpoint-save', 'checkpoint-read'], detail: 'Use <command> --help or docs/SESSION.md for session policy.' };
  const hint = flag => { const index = args.indexOf(flag); const value = args[index+1]; return index >= 0 && typeof value === 'string' && !value.startsWith('--') ? value : undefined; };
  return withTelemetry(path.resolve(hint('--root') ?? '.'), { session:hint('--session'), host:hint('--host') },
    () => observe('cli.command', () => parse(action,args), () => ({ command:action }), { failureCode:'INVALID_INPUT' }));
}
function parse(action,args) {
  const allowed = ['root', 'intent', 'file', 'host', 'delegate', 'task', 'risk', 'operation', 'params', 'session', 'data', 'module', 'term', 'path', 'max-files', 'max-matches', 'max-bytes', 'available-command', 'available-mcp-tool', 'bundle', 'budget-bytes', 'verification-mode', 'json', 'max-output-bytes'];
  const options = { file: [], risk: [], term: [], path: [] };
  const repeat = ['file', 'risk', 'term', 'path', 'available-command', 'available-mcp-tool'];
  for (let index = 0; index < args.length; index++) {
    const arg = args[index];
    if (!arg.startsWith('--') || !allowed.includes(arg.slice(2))) throw new Error(`unknown argument: ${arg}`);
    const name = arg.slice(2);
    if (['delegate', 'json'].includes(name)) { if (options[name]) throw new Error(`duplicate option --${name}`); options[name] = true; continue; }
    const value = args[++index];
    if (value === undefined || value.startsWith('--')) throw new Error(`missing value for ${arg}`);
    if (repeat.includes(name)) (options[name] ??= []).push(value);
    else if (Object.hasOwn(options, name)) throw new Error(`duplicate option ${arg}`);
    else options[name] = value;
  }
  return execute(action, options, path.resolve(options.root ?? '.'));
}

if (process.argv[1] && fileURLToPath(import.meta.url) === fs.realpathSync(process.argv[1])) {
  try {
    const result = run(process.argv.slice(2));
    process.stdout.write(process.argv[2] === 'session' && !process.argv.includes('--json') && !process.argv.includes('--help') ? renderSessionStatus(result) : `${JSON.stringify(result, null, process.argv[2] === 'locate' ? undefined : 2)}\n`);
  }
  catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 2; }
  finally { await flushTelemetry(); }
}
