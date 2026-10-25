#!/usr/bin/env node
import { flushTelemetry, forwardTelemetry, withTelemetry, observe } from '../application/telemetry/observe.mjs';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { execute } from './requests.mjs';
import { renderCommand } from './render.mjs';
import { nativeCommand, bundleRoot } from '../platform/index.mjs';

const native = new Set(['settings', 'operation', 'prepare-services', 'model-advice', 'lookup', 'checkpoint-save', 'checkpoint-read', 'session', 'session-advice', 'session-attempt', 'task-start', 'task-step', 'task-offer', 'task-decision', 'task-action-check', 'workspace-open', 'workspace-write', 'workspace-promote', 'workspace-release', 'workspace-clean']); // complete use cases in the native runtime

const help = {
  'worker-status': { usage: 'worker-status --root project --session id [--task id]', mode: 'shared task-tree reservations and host observations; no dispatch' },
  'task-feature': { usage: 'task-feature --session id --feature feature-id --feature-status used|eligible-but-skipped|disabled|unsupported|not-applicable|failed --reason reason --evidence reference', mode: 'explicit evaluation classification only; normal delivery uses runtime observations and leaves missing evidence unknown' },
  'task-features': { usage: 'task-features --session id [--task id]', mode: 'all feature outcomes, provenance, missing evidence and stale-source markers' },
  'telemetry-capture': { usage: 'telemetry-capture --root project --session id', mode: 'explicit private immutable report retention; eight captures / 64 KiB; no automatic eviction' },
  'telemetry-forget': { usage: 'telemetry-forget --root project --capture id', mode: 'explicitly remove one retained report; ordinary CLI telemetry still applies' },
  'telemetry-report': { usage: 'telemetry-report [--root project] [--session id | --capture id]', mode: 'bounded retained observation counts, loss reports and storage limits; no semantic or native execution claims' },
};

export function run(argv) {
  const [action, ...args] = argv;
  // Session guidance is accounted against this installed bundle unless the caller names roots.
  if (native.has(action)) { const outcome = nativeCommand(['session', 'session-advice', 'task-step', 'task-offer', 'task-action-check'].includes(action) && !args.includes('--bundle') ? [...argv, '--bundle', bundleRoot()] : argv, { structured: true }); forwardTelemetry(outcome.export); if (outcome.exitCode) throw new Error(outcome.stderr.trimEnd()); return JSON.parse(outcome.stdout); }
  if (['locate', 'context-report'].includes(action) && args.includes('--help')) { const outcome = nativeCommand(argv); return JSON.parse(outcome.stdout); }
  if (action === 'help' || action === '--help' || args.includes('--help')) return { ...(help[action] ?? { commands: ['workspace-open', 'workspace-write', 'workspace-promote', 'workspace-release', 'workspace-clean', 'worker-status', 'task-feature', 'task-features', 'telemetry-capture', 'telemetry-forget', 'telemetry-report', 'task-offer', 'task-decision', 'task-action-check', 'task-start', 'task-step', 'settings', 'route', 'context-report', 'session', 'session-advice', 'session-attempt', 'model-advice', 'lookup', 'locate', 'prepare-services', 'operation', 'checkpoint-save', 'checkpoint-read'], detail: 'Use <command> --help or docs/SESSION.md for session policy.' }), output: 'Routine receipts are compact by default; --json or --verbose returns the full structured result.' };
  const hint = flag => { const index = args.indexOf(flag); const value = args[index+1]; return index >= 0 && typeof value === 'string' && !value.startsWith('--') ? value : undefined; };
  return withTelemetry(path.resolve(hint('--root') ?? '.'), { session:hint('--session'), host:hint('--host') },
    () => observe('cli.command', () => parse(action,args), () => ({ command:action }), { failureCode:'INVALID_INPUT' }));
}
function parse(action,args) {
  const allowed = ['contract', 'acceptance-evidence', 'expected-evidence-digest', 'expected-task-contract', 'repository', 'feature', 'feature-status', 'reason', 'capture', 'review-authorization', 'action', 'scope', 'benefit', 'offer', 'decision', 'role', 'prior-authorization', 'facts', 'goal', 'stage', 'target', 'approach', 'evidence', 'outcome', 'result', 'next', 'profile', 'complete', 'preferences', 'root', 'intent', 'file', 'host', 'delegate', 'task', 'risk', 'operation', 'params', 'session', 'data', 'module', 'term', 'path', 'max-files', 'max-matches', 'max-bytes', 'available-command', 'available-mcp-tool', 'bundle', 'budget-bytes', 'verification-mode', 'json', 'verbose', 'max-output-bytes'];
  const options = { file: [], risk: [], term: [], path: [] };
  const repeat = ['repository', 'file', 'risk', 'term', 'path', 'available-command', 'available-mcp-tool', 'bundle'];
  for (let index = 0; index < args.length; index++) {
    const arg = args[index];
    if (!arg.startsWith('--') || !allowed.includes(arg.slice(2))) throw new Error(`unknown argument: ${arg}`);
    const name = arg.slice(2);
    if (['delegate', 'json', 'verbose', 'complete'].includes(name)) { if (options[name]) throw new Error(`duplicate option --${name}`); options[name] = true; continue; }
    const value = args[++index];
    if (value === undefined || value.startsWith('--')) throw new Error(`missing value for ${arg}`);
    if (repeat.includes(name)) (options[name] ??= []).push(value);
    else if (Object.hasOwn(options, name)) throw new Error(`duplicate option ${arg}`);
    else options[name] = value;
  }
  if (options.contract && action !== 'task-start') throw new Error('--contract is supported by task-start');
  if ((options['acceptance-evidence'] || options['expected-task-contract'] || options['expected-evidence-digest']) && action !== 'task-step') throw new Error('structured evidence options require task-step');
  if (options.repository && !['task-start', 'checkpoint-save'].includes(action)) throw new Error('--repository is supported by task-start and checkpoint-save; other commands reuse the saved scope');
  return execute(action, options, path.resolve(options.root ?? '.'));
}

if (process.argv[1] && fileURLToPath(import.meta.url) === fs.realpathSync(process.argv[1])) {
  try {
    const result = run(process.argv.slice(2));
    process.stdout.write(renderCommand(process.argv[2], result, { json: process.argv.includes('--json'), verbose: process.argv.includes('--verbose'), help: process.argv.includes('--help') }));
  }
  catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 2; }
  finally { await flushTelemetry(); }
}
