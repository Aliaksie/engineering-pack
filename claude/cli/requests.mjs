import { readProject } from '../application/project/configuration.mjs';
import { lookup, route } from '../application/routing/select.mjs';
import { planOperation } from '../application/services.mjs';
import { recoverCheckpoint, saveCheckpoint } from '../application/checkpoint.mjs';
import { discover } from '../application/discovery/discover.mjs';
import { sessionStatus } from '../application/session/status.mjs';
import { updateSessionState, addActivity, recordAttempt } from '../application/session/activity.mjs';
import { prepareServices } from '../application/services.mjs';
import { contextReport } from '../application/session/context.mjs';
import { readSessionInput, bundleRoot } from '../platform/index.mjs';
import { modelAdvice } from '../application/session/models.mjs';


export function execute(action, options, root) {

  if (action === 'locate') {
    const limits = {};
    for (const [flag, field] of [['max-files', 'maxFiles'], ['max-matches', 'maxMatches'], ['max-bytes', 'maxBytes'], ['max-output-bytes', 'maxOutputBytes']]) {
      if (options[flag] !== undefined) limits[field] = Number(options[flag]);
    }
    const request = options.data ? readSessionInput(options.data) : { terms: options.term, paths: options.path, ...limits };
    return discover(root, request, { session: options.session, task: options.task });
  }
  if (action === 'prepare-services') return prepareServices(readProject(root), { availableCommands: options['available-command'], availableMcpTools: options['available-mcp-tool'] });
  if (['session', 'session-advice'].includes(action)) return sessionStatus(options.data ? readSessionInput(options.data) : {}, readProject(root), { root, session: options.session, task: options.task, bundle: options.bundle });
  if (action === 'session-attempt') {
    if (!options.data || !options.session || !options.task) throw new Error('session-attempt requires --session, --task and --data');
    const state = recordAttempt(root, options.session, options.task, readSessionInput(options.data));
    return { recorded: state.attempts.at(-1), authorization: 'local receipt only; no retry, model switch or command was executed' };
  }
  const activity = (result, values) => {
    if (options.session && options.task) {
      try { updateSessionState(root, options.session, options.task, state => addActivity(state, values)); }
      catch (error) { return { ...result, activityDiagnostic: error.message }; }
    }
    return result;
  };
  if (action === 'model-advice') {
    if (!options.data) throw new Error('model-advice requires --data <request.json>');
    return modelAdvice(readSessionInput(options.data), readProject(root));
  }
  if (action === 'lookup') return lookup(readProject(root), { file: options.file[0], module: options.module });
  if (action === 'context-report') {
    const plan = route({ intent: options.intent, files: options.file, risks: options.risk, project: readProject(root), host: options.host, verificationMode: options['verification-mode'] });
    const bundle = options.bundle ?? bundleRoot();
    const report = contextReport(bundle, plan, { budgetBytes: options['budget-bytes'] === undefined ? undefined : Number(options['budget-bytes']) });
    return activity(report, { filesRequested: report.totals.files, referencesRequested: plan.references.length });
  }
  if (action === 'route') {
    const plan = route({ intent: options.intent, files: options.file, risks: options.risk, project: readProject(root), host: options.host, delegate: options.delegate, task: options.task, verificationMode: options['verification-mode'] });
    return activity(plan, { routingDecisions: 1 });
  }
  if (action === 'operation') return planOperation(readProject(root), options.operation, JSON.parse(options.params ?? '{}'));
  if (action === 'checkpoint-save') {
    const metadata = options.data ? readSessionInput(options.data) : {};
    const saved = saveCheckpoint(root, options.session ?? 'manual', { ...metadata, ...(options.task ? { taskId: options.task } : {}) });
    return activity(saved, { checkpoints: 1 });
  }
  if (action === 'checkpoint-read') return recoverCheckpoint(root, options.session ?? 'manual');
  throw new Error('usage: cli.mjs <route|context-report|session|session-advice|session-attempt|model-advice|lookup|locate|prepare-services|operation|checkpoint-save|checkpoint-read> [--root project] [options]');
}
