#!/usr/bin/env node
import { flushTelemetry, forwardTelemetry } from '../application/telemetry/observe.mjs';
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';
import { renderCommand } from './render.mjs';
import { nativeCommand, bundleRoot } from '../platform/index.mjs';

const bundled = ['context-report', 'session', 'session-advice', 'task-step', 'task-offer', 'task-action-check'];
const commands = ['workspace-open', 'workspace-write', 'workspace-promote', 'workspace-release', 'workspace-clean', 'grounded-context', 'worker-status', 'task-feature', 'task-features', 'telemetry-capture', 'telemetry-forget', 'telemetry-report', 'task-offer', 'task-decision', 'task-action-check', 'task-start', 'task-step', 'settings', 'route', 'context-report', 'session', 'session-advice', 'session-attempt', 'model-advice', 'lookup', 'locate', 'prepare-services', 'operation', 'checkpoint-save', 'checkpoint-read'];

// Every command runs in the native runtime; this entry only adds the installed bundle.
export function run(argv) {
  const [action, ...args] = argv;
  if (action === 'help' || action === '--help' || (!commands.includes(action) && args.includes('--help'))) return { commands, detail: 'Use <command> --help or docs/SESSION.md for session policy.', output: 'Routine receipts are compact by default; --json or --verbose returns the full structured result.' };
  // Session guidance is accounted against this installed bundle unless the caller names roots.
  const outcome = nativeCommand(bundled.includes(action) && !args.includes('--bundle') ? [...argv, '--bundle', bundleRoot()] : argv, { structured: true });
  forwardTelemetry(outcome.export);
  if (outcome.exitCode) throw Object.assign(new Error(outcome.stderr.trimEnd()), { code: outcome.errorCode ?? undefined });
  return JSON.parse(outcome.stdout);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === fs.realpathSync(process.argv[1])) {
  try {
    const result = run(process.argv.slice(2));
    process.stdout.write(renderCommand(process.argv[2], result, { json: process.argv.includes('--json'), verbose: process.argv.includes('--verbose'), help: process.argv.includes('--help') }));
  }
  catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 2; }
  finally { await flushTelemetry(); }
}
