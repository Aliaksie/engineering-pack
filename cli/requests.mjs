import { startTask, stepTask } from '../application/session/task.mjs';
import { withTelemetry, observe, diagnostic } from '../application/telemetry/observe.mjs';
import { readProject, readInput, readEffectiveSettings } from '../application/project/configuration.mjs';
import { lookup, route } from '../application/routing/select.mjs';
import { planOperation } from '../application/services.mjs';
import { recoverCheckpoint, saveCheckpoint } from '../application/checkpoint.mjs';
import { discover } from '../application/discovery/discover.mjs';
import { sessionStatus } from '../application/session/status.mjs';
import { updateSessionState, addActivity, recordAttempt, activeTask } from '../application/session/activity.mjs';
import { prepareServices } from '../application/services.mjs';
import { contextReport } from '../application/session/context.mjs';
import { bundleRoot } from '../platform/index.mjs';
import { modelAdvice } from '../application/session/models.mjs';


export function execute(action, options, root) {
  return withTelemetry(root, { session: options.session, host: options.host }, () => observe('cli.command', () => executeRequest(action,options,root), () => ({ command:action }), { failureCode:'INVALID_INPUT', reuse:true }));
}
function executeRequest(action, options, root) {
  if (options.session && !options.task && ['route', 'context-report', 'locate', 'session-attempt', 'task-step'].includes(action)) {
    try { options = { ...options, task: activeTask(root, options.session) }; }
    catch (error) {
      if (['task-step', 'session-attempt'].includes(action)) throw error;
      diagnostic('state', 'STATE_UNAVAILABLE');
    }
  }
  if (action === 'task-start') {
    if (!options.session || !options.task || !options.goal) throw new Error('task-start requires --session, --task and --goal');
    return startTask(root, options.session, options.task, { goal: options.goal, ...(options.stage ? { stage: options.stage } : {}), ...(options.file?.length ? { files: options.file } : {}) }, { host: options.host, preferencesFile: options.preferences });
  }
  if (action === 'task-step') {
    if (!options.session || !options.task) throw new Error('task-step requires --session and an explicit --task or a task-start binding');
    return stepTask(root, options.session, options.task, { stage: options.stage, target: options.target, approach: options.approach, evidence: options.evidence, outcome: options.outcome, result: options.result, nextStep: options.next,
      ...(options.profile !== undefined ? { profile: options.profile } : {}), complete: options.complete === true }, { host: options.host, preferencesFile: options.preferences, bundle: options.bundle });
  }

  if (action === 'settings') return readEffectiveSettings(root, { preferencesFile: options.preferences, overrides: options.data ? readInput(options.data) : {} });
  if (action === 'locate') {
    const limits = {};
    for (const [flag, field] of [['max-files', 'maxFiles'], ['max-matches', 'maxMatches'], ['max-bytes', 'maxBytes'], ['max-output-bytes', 'maxOutputBytes']]) {
      if (options[flag] !== undefined) limits[field] = Number(options[flag]);
    }
    const request = options.data ? readInput(options.data) : { terms: options.term, paths: options.path, ...limits };
    return discover(root, request, { session: options.session, task: options.task, preferencesFile: options.preferences });
  }
  if (action === 'prepare-services') return prepareServices(readProject(root, { preferencesFile: options.preferences }), { availableCommands: options['available-command'], availableMcpTools: options['available-mcp-tool'] });
  if (['session', 'session-advice'].includes(action)) return sessionStatus(options.data ? readInput(options.data) : {}, readProject(root, { preferencesFile: options.preferences }), { root, session: options.session, task: options.task, bundle: options.bundle });
  if (action === 'session-attempt') {
    if (!options.data || !options.session || !options.task) throw new Error('session-attempt requires --session, --task and --data');
    const state = recordAttempt(root, options.session, options.task, readInput(options.data), { preferencesFile: options.preferences });
    return { recorded: state.attempts.at(-1), authorization: 'local receipt only; no retry, model switch or command was executed' };
  }
  const activity = (result, values) => {
    if (options.session && options.task) {
      try { updateSessionState(root, options.session, options.task, state => addActivity(state, values)); }
      catch (error) { diagnostic('state','STATE_UNAVAILABLE'); return { ...result, activityDiagnostic: error.message }; }
    }
    return result;
  };
  if (action === 'model-advice') {
    if (!options.data) throw new Error('model-advice requires --data <request.json>');
    return modelAdvice(readInput(options.data), readProject(root, { preferencesFile: options.preferences }));
  }
  if (action === 'lookup') return lookup(readProject(root, { preferencesFile: options.preferences }), { file: options.file[0], module: options.module });
  if (action === 'context-report') {
    const plan = route({ intent: options.intent, files: options.file, risks: options.risk, project: readProject(root, { preferencesFile: options.preferences }), host: options.host, verificationMode: options['verification-mode'] });
    const bundle = options.bundle ?? bundleRoot();
    const report = contextReport(bundle, plan, { budgetBytes: options['budget-bytes'] === undefined ? undefined : Number(options['budget-bytes']) });
    return activity(report, { filesRequested: report.totals.files, referencesRequested: plan.references.length });
  }
  if (action === 'route') {
    const plan = route({ intent: options.intent, files: options.file, risks: options.risk, project: readProject(root, { preferencesFile: options.preferences }), host: options.host, delegate: options.delegate, task: options.task, verificationMode: options['verification-mode'] });
    return activity(plan, { routingDecisions: 1 });
  }
  if (action === 'operation') return planOperation(readProject(root, { preferencesFile: options.preferences }), options.operation, JSON.parse(options.params ?? '{}'));
  if (action === 'checkpoint-save') {
    const metadata = options.data ? readInput(options.data) : {};
    let task = options.task ?? metadata.taskId;
    if (!task && options.session) {
      try { task = activeTask(root, options.session); } catch { diagnostic('state', 'STATE_UNAVAILABLE'); }
    }
    options = { ...options, task };
    const saved = saveCheckpoint(root, options.session ?? 'manual', { ...metadata, ...(options.task ? { taskId: options.task } : {}) });
    return activity(saved, { checkpoints: 1 });
  }
  if (action === 'checkpoint-read') return recoverCheckpoint(root, options.session ?? 'manual');
  throw new Error('usage: cli.mjs <route|context-report|session|session-advice|session-attempt|model-advice|lookup|locate|prepare-services|operation|checkpoint-save|checkpoint-read> [--root project] [options]');
}
