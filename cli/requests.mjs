import { telemetryReport } from '../application/telemetry/report.mjs';
import { captureTelemetry, readTelemetryCapture, forgetTelemetryCapture } from '../application/telemetry/capture.mjs';
import { workerStatus } from '../application/session/workers.mjs';
import { withTelemetry, observe, diagnostic, forwardTelemetry, commandTelemetry } from '../application/telemetry/observe.mjs';
import { locateCommand } from '../application/discovery/discover.mjs';
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
  if (options.session && !options.task && ['worker-status', 'route', 'context-report', 'locate'].includes(action)) {
    try { options = { ...options, task: activeTask(root, options.session) }; }
    catch { diagnostic('state', 'STATE_UNAVAILABLE'); }
  }
  if (action === 'worker-status') {
    if (!options.session || !options.task) throw new Error('task outcome/worker view requires --session and a task binding or explicit --task');
    return workerStatus(root, options.session, options.task);
  }
  if (action === 'locate') return locateCommand(root, options);
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
  throw new Error('usage: cli.mjs <telemetry-capture|telemetry-forget|telemetry-report|route|context-report|locate> [--root project] [options]');
}
