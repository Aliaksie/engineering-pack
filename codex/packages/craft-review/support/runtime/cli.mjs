#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { readProject } from './project.mjs';
import { lookup, route } from './router.mjs';
import { planOperation } from './bindings.mjs';
import { recoverCheckpoint, saveCheckpoint } from './checkpoint.mjs';
import { locate } from './locate.mjs';
import { prepareServices } from './services.mjs';
import { contextReport } from './context.mjs';
import { sessionAdvice, readSessionInput } from './session.mjs';
import { modelAdvice } from './model-advice.mjs';

export function run(argv) {
  const [action, ...args] = argv;
  const allowed = ['root', 'intent', 'file', 'host', 'delegate', 'task', 'risk', 'operation', 'params', 'session', 'data', 'module', 'term', 'path', 'max-files', 'max-matches', 'max-bytes', 'available-command', 'available-mcp-tool', 'bundle', 'budget-bytes', 'verification-mode'];
  const options = { file: [], risk: [], term: [], path: [] };
  const repeat = ['file', 'risk', 'term', 'path', 'available-command', 'available-mcp-tool'];
  for (let index = 0; index < args.length; index++) {
    const arg = args[index];
    if (!arg.startsWith('--') || !allowed.includes(arg.slice(2))) throw new Error(`unknown argument: ${arg}`);
    const name = arg.slice(2);
    if (name === 'delegate') { options.delegate = true; continue; }
    const value = args[++index];
    if (value === undefined || value.startsWith('--')) throw new Error(`missing value for ${arg}`);
    if (repeat.includes(name)) (options[name] ??= []).push(value);
    else if (Object.hasOwn(options, name)) throw new Error(`duplicate option ${arg}`);
    else options[name] = value;
  }
  const root = path.resolve(options.root ?? '.');
  if (action === 'locate') {
    const limits = {};
    for (const [flag, field] of [['max-files', 'maxFiles'], ['max-matches', 'maxMatches'], ['max-bytes', 'maxBytes']]) {
      if (options[flag] !== undefined) limits[field] = Number(options[flag]);
    }
    return locate(root, { terms: options.term, paths: options.path, ...limits });
  }
  if (action === 'prepare-services') return prepareServices(readProject(root), { availableCommands: options['available-command'], availableMcpTools: options['available-mcp-tool'] });
  if (action === 'session-advice') return sessionAdvice(options.data ? readSessionInput(options.data) : {}, readProject(root));
  if (action === 'model-advice') {
    if (!options.data) throw new Error('model-advice requires --data <request.json>');
    return modelAdvice(readSessionInput(options.data), readProject(root));
  }
  if (action === 'lookup') return lookup(readProject(root), { file: options.file[0], module: options.module });
  if (action === 'context-report') {
    const plan = route({ intent: options.intent, files: options.file, risks: options.risk, project: readProject(root), host: options.host, verificationMode: options['verification-mode'] });
    // Source/full bundles have runtime/; standalone packages have support/runtime/.
    const runtimeRoot = path.dirname(fileURLToPath(import.meta.url));
    const bundle = options.bundle ?? path.resolve(runtimeRoot, path.basename(path.dirname(runtimeRoot)) === 'support' ? '../..' : '..');
    return contextReport(bundle, plan, { budgetBytes: options['budget-bytes'] === undefined ? undefined : Number(options['budget-bytes']) });
  }
  if (action === 'route') return route({ intent: options.intent, files: options.file, risks: options.risk, project: readProject(root), host: options.host, delegate: options.delegate, task: options.task, verificationMode: options['verification-mode'] });
  if (action === 'operation') return planOperation(readProject(root), options.operation, JSON.parse(options.params ?? '{}'));
  if (action === 'checkpoint-save') return saveCheckpoint(root, options.session ?? 'manual', options.data ? JSON.parse(fs.readFileSync(path.resolve(options.data), 'utf8')) : {});
  if (action === 'checkpoint-read') return recoverCheckpoint(root, options.session ?? 'manual');
  throw new Error('usage: cli.mjs <route|context-report|session-advice|model-advice|lookup|locate|prepare-services|operation|checkpoint-save|checkpoint-read> [--root project] [options]');
}
if (process.argv[1] && fileURLToPath(import.meta.url) === fs.realpathSync(process.argv[1])) {
  try { process.stdout.write(`${JSON.stringify(run(process.argv.slice(2)), null, 2)}\n`); }
  catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 2; }
}
