import { stepTask, taskAction } from '../application/session/task.mjs';
import { telemetryReport } from '../application/telemetry/report.mjs';
import { captureTelemetry, readTelemetryCapture, forgetTelemetryCapture } from '../application/telemetry/capture.mjs';
import { taskFeatures } from '../application/session/features.mjs';
import { workerStatus } from '../application/session/workers.mjs';
import { withTelemetry, observe, diagnostic, forwardTelemetry, commandTelemetry } from '../application/telemetry/observe.mjs';
import { readProject, readInput } from '../application/project/configuration.mjs';
import { locateCommand } from '../application/discovery/discover.mjs';
import { sessionStatus } from '../application/session/status.mjs';
import { updateSessionState, addActivity, activeTask } from '../application/session/activity.mjs';
import { bundleRoot, nativeCommand } from '../platform/index.mjs';
export function execute(action, options, root) {
  return withTelemetry(root, { session: options.session, host: options.host }, () => observe('cli.command', () => executeRequest(action,options,root), () => ({ command:action }), { failureCode:'INVALID_INPUT', reuse:true }));
}
function executeRequest(action, options, root) {
  if (action === 'telemetry-capture') return captureTelemetry(root, { session: options.session });
  if (action === 'telemetry-forget') return forgetTelemetryCapture(root, options.capture);
  if (action === 'telemetry-report') {
    if (options.capture && options.session) throw new Error('choose --capture or --session for telemetry-report');
    return options.capture ? readTelemetryCapture(root, options.capture) : telemetryReport(root, { session: options.session });
  }
  if (options.session && !options.task && ['worker-status', 'task-features', 'task-feature', 'route', 'context-report', 'locate', 'task-step', 'task-offer', 'task-decision', 'task-action-check'].includes(action)) {
    try { options = { ...options, task: activeTask(root, options.session) }; }
    catch (error) {
      if (['task-step', 'task-offer', 'task-decision', 'task-action-check'].includes(action)) throw error;
      diagnostic('state', 'STATE_UNAVAILABLE');
    }
  }
  if (['task-features', 'task-feature', 'worker-status'].includes(action)) {
    if (!options.session || !options.task) throw new Error('task outcome/worker view requires --session and a task binding or explicit --task');
    if (action === 'worker-status') return workerStatus(root, options.session, options.task);
    return taskFeatures(root, options.session, options.task, action === 'task-feature'
      ? { feature: options.feature, status: options['feature-status'], reason: options.reason, evidence: options.evidence } : undefined);
  }
  if (['task-offer', 'task-decision', 'task-action-check'].includes(action)) {
    if (!options.session || !options.task) throw new Error('action recommendation requires --session and a task-start binding');
    const input = action === 'task-offer' ? { action: options.action, scope: options.scope, benefit: options.benefit, evidence: options.evidence,
      ...(options.role !== undefined ? { role: options.role } : {}), ...(options.profile !== undefined ? { profile: options.profile } : {}),
      ...(options['prior-authorization'] ? { priorAuthorization: options['prior-authorization'] } : {}) }
      : { id: options.offer, ...(action === 'task-decision' ? { decision: options.decision, evidence: options.evidence } : {}) };
    return taskAction(root, options.session, options.task, { 'task-offer': 'action-propose', 'task-decision': 'action-decide', 'task-action-check': 'action-check' }[action], input,
      options.data ? readInput(options.data) : {}, { preferencesFile: options.preferences, bundle: options.bundle });
  }
  if (action === 'task-step') {
    if (!options.session || !options.task) throw new Error('task-step requires --session and an explicit --task or a task-start binding');
    return stepTask(root, options.session, options.task, { stage: options.stage, target: options.target, approach: options.approach, evidence: options.evidence, outcome: options.outcome, result: options.result, nextStep: options.next,
      ...(options['acceptance-evidence'] ? { acceptanceEvidence: readInput(options['acceptance-evidence']), expectedTaskContract: options['expected-task-contract'], expectedEvidenceDigest: options['expected-evidence-digest'] } : {}),
      ...(options.profile !== undefined ? { profile: options.profile } : {}), complete: options.complete === true }, { host: options.host, preferencesFile: options.preferences, bundle: options.bundle });
  }

  if (action === 'locate') return locateCommand(root, options);
  if (['session', 'session-advice'].includes(action)) return sessionStatus(options.data ? readInput(options.data) : {}, readProject(root, { preferencesFile: options.preferences }), { root, session: options.session, task: options.task, bundle: options.bundle });
  const activity = (result, values) => {
    if (options.session && options.task) {
      try { updateSessionState(root, options.session, options.task, state => addActivity(state, values)); }
      catch (error) { diagnostic('state','STATE_UNAVAILABLE'); return { ...result, activityDiagnostic: error.message }; }
    }
    return result;
  };
  // Session binding/activity remain with their W2 owner; each use case runs once in Rust.
  const forward = (command, values) => {
    const argv = Object.entries({ ...values, root }).filter(([, value]) => value !== undefined).flatMap(([key, value]) => (Array.isArray(value) ? value : [value]).flatMap(item => item === true ? [`--${key}`] : [`--${key}`, item]));
    const outcome = nativeCommand([command, ...argv], { telemetry: commandTelemetry() }); forwardTelemetry(outcome.export);
    if (outcome.exitCode) throw Object.assign(new Error(outcome.stderr.trimEnd()), { code: outcome.errorCode });
    return outcome;
  };
  if (action === 'context-report') {
    const outcome = forward('context-report', { ...options, bundle: options.bundle ?? [bundleRoot()] });
    return activity(JSON.parse(outcome.stdout), outcome.activity);
  }
  if (action === 'route') return activity(JSON.parse(forward('route', options).stdout), { routingDecisions: 1 });
  throw new Error('usage: cli.mjs <telemetry-capture|telemetry-forget|telemetry-report|route|context-report|session|session-advice|locate> [--root project] [options]');
}
