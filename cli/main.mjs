#!/usr/bin/env node
import { flushTelemetry, forwardTelemetry, withTelemetry, observe } from '../application/telemetry/observe.mjs';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { execute } from './requests.mjs';
import { renderCommand } from './render.mjs';
import { nativeCommand } from '../platform/index.mjs';

const native = new Set(['settings', 'operation', 'prepare-services']); // complete use cases in the native runtime

const help = {
  'workspace-open': { usage: 'workspace-open --root project --session id [--task id] [--operation scope]', mode: 'lazily allocate bounded private scratch for the active task; no host temp/cache changes' },
  'workspace-write': { usage: 'workspace-write --session id [--operation scope] --data artifact.json', mode: 'write a new bounded scratch file from {name,content}; existing files are never overwritten' },
  'workspace-promote': { usage: 'workspace-promote --session id [--operation scope] --data promotion.json', mode: 'intentionally retain bounded evidence from {name,purpose}; never promote secrets or transcripts' },
  'workspace-release': { usage: 'workspace-release --session id [--operation scope]', mode: 'release the current owner operation; task completion cleans disposable scratch when no other owner is active' },
  'workspace-clean': { usage: 'workspace-clean --root project', mode: 'reclaim closed or stale abandoned storage; live or unverified owners are retained' },
  'worker-status': { usage: 'worker-status --root project --session id [--task id]', mode: 'shared task-tree reservations and host observations; no dispatch' },
  'task-feature': { usage: 'task-feature --session id --feature feature-id --feature-status used|eligible-but-skipped|disabled|unsupported|not-applicable|failed --reason reason --evidence reference', mode: 'explicit evaluation classification only; normal delivery uses runtime observations and leaves missing evidence unknown' },
  'task-features': { usage: 'task-features --session id [--task id]', mode: 'all feature outcomes, provenance, missing evidence and stale-source markers' },
  'telemetry-capture': { usage: 'telemetry-capture --root project --session id', mode: 'explicit private immutable report retention; eight captures / 64 KiB; no automatic eviction' },
  'telemetry-forget': { usage: 'telemetry-forget --root project --capture id', mode: 'explicitly remove one retained report; ordinary CLI telemetry still applies' },
  'telemetry-report': { usage: 'telemetry-report [--root project] [--session id | --capture id]', mode: 'bounded retained observation counts, loss reports and storage limits; no semantic or native execution claims' },
  'task-offer': { usage: 'task-offer --session id --action review|delegate|compact --scope scope --benefit benefit --evidence evidence-id [--data observations.json] [--prior-authorization evidence-id]', mode: 'evaluate and record a recommendation; no dispatch' },
  'task-decision': { usage: 'task-decision --session id --offer proposal-id --decision approved|declined --evidence user-decision-id', mode: 'record an explicit scoped user decision; no action execution' },
  'task-action-check': { usage: 'task-action-check --session id --offer proposal-id --data fresh-observations.json', mode: 'revalidate source, approval, policy, support and capacity; no dispatch' },
  'task-start': { usage: 'task-start --session id --task stable-id --goal "outcome" [--stage implement] [--file src/path] [--repository relative/repo ...] [--contract contract.json]', mode: 'bind task identity and optional selected repository roots within the workspace; route guidance and snapshot selected Git worktrees' },
  'task-step': { usage: 'task-step --session id --stage verify --target acceptance:id --approach bounded-check --evidence evidence-id --outcome passed --result "observed result" --next "next step" [--acceptance-evidence ledger.json --expected-task-contract digest --expected-evidence-digest digest] [--complete]', mode: 'agent-declared semantic receipt, bounded continuation and next-step advice; no action dispatch' },
  session: { usage: 'session [--root project] [--session id --task task-id] [--data task.json] [--json]',
    taskFields: ['id', 'revision', 'stage', 'operation', 'status', 'files', 'safeBoundary', 'phaseChanged', 'almostComplete', 'compacted', 'staleAssumptions', 'risks', 'nextStep'], mode: 'recommendation-only' },
  'context-report': { usage: 'context-report --intent implement [--file src/main.py] [--bundle package-root ...]', mode: 'selected guidance accounting; repeat --bundle for verified separately installed packages; no cache discovery or resource loading' },
  locate: { usage: 'locate --root project --term Foo --term Bar [--path src] [--session id --task task-id] [--max-output-bytes 32768]',
    data: 'Alternatively --data request.json: terms or independent requests {id,term,dependsOn}, paths, maxFiles, maxMatches, maxBytes, maxOutputBytes', mode: 'bounded literal scan; optional local reuse receipts' },
  'session-attempt': { usage: 'session-attempt --root project --session id --task task-id --data attempt.json',
    example: { operation: 'implement', target: 'acceptance:regression', approach: 'bounded-fix', evidence: 'artifacts/check.json', outcome: 'passed' },
    outcomes: ['passed', 'quality-failure', 'permission-denied', 'tool-unavailable', 'blocked', 'prerequisite-resolved'], optional: ['profile', 'maxAttempts', 'escalationReason'], mode: 'receipt only; no retry execution' },
  'checkpoint-read': { usage: 'checkpoint-read --root project --session id', mode: 'read-only recovery comparison' },
  'checkpoint-save': { usage: 'checkpoint-save --root project --session id [--task task-id] [--repository relative/repo ...] [--contract contract.json] --data handoff.json',
    continuationFields: ['goal', 'currentState', 'completed', 'approach', 'evidence', 'filesChanged', 'validation', 'remainingWork', 'blockers', 'nextStep'], mode: 'explicit local checkpoint write' }
};

export function run(argv) {
  const [action, ...args] = argv;
  if (native.has(action)) { const outcome = nativeCommand(argv); forwardTelemetry(outcome.export); if (outcome.exitCode) throw new Error(outcome.stderr.trimEnd()); return JSON.parse(outcome.stdout); }
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
