import { activeTask } from '../session/activity.mjs';
import { workspaceBoundary } from '../workspace.mjs';
import { withTelemetry, flushTelemetry, observe, diagnostic } from '../telemetry/observe.mjs';
import { prepare as prepareLifecycleObservation } from '../telemetry/lifecycle.mjs';
export { flushTelemetry };
import { projectRoot, relativePath } from '../../platform/index.mjs';
import { readProject } from '../project/configuration.mjs';
import { route } from '../routing/select.mjs';
import { prepareServices } from '../services.mjs';
import { decide } from '../engine-client.mjs';
import { readCheckpoint, recoverCheckpoint, saveCheckpoint } from '../checkpoint.mjs';
import { invalidateRoutingHint } from './hints.mjs';
export { reuseRoutingHint, commitRoutingHint } from './hints.mjs';

export const hookDiagnostic = (reasonCode, attributes) => diagnostic('host',reasonCode,attributes);
export const observeHook = (event, callback) => withTelemetry(projectRoot(event.cwd), { session:event.session, host:event.host, packageName:event.packageName },
  () => observe('host.hook', callback, result => ({ lifecycle:event.event, inputStatus:event.inputStatus, decision:result?.decision }),
    { failureCode:'HOOK_FAILED', outcome:result => result?.failed ? 'failure' : 'success' }));
export const observeNativeSession = (root, host, callback, packageName) => withTelemetry(root, { host, packageName },
  () => observe('host.session', callback, () => ({}), { failureCode:'INVALID_INPUT', outcome:result => result?.failed ? 'failure' : 'success' }));
export function prepareEvent(event) {
  return withTelemetry(projectRoot(event.cwd), { session: event.session, host: event.host }, () => prepare(event));
}
function prepare(event) {
  prepareLifecycleObservation(event);
  const root = projectRoot(event.cwd);
  const project = readProject(root);
  let task;
  try { task = activeTask(root, event.session); }
  catch { diagnostic('state', 'STATE_UNAVAILABLE'); }
  const taskIdentity = { session: event.session ?? null, task: task ?? null, source: task ? 'session-binding' : 'unavailable' };
  const result = { ...event, project, routingRoot: root, taskIdentity };
  if (event.event === 'session-start') result.workspace = workspaceBoundary(root, event.session, task, 'maintain', project);
  else if (task && event.event === 'before-prompt') result.workspace = workspaceBoundary(root, event.session, task, 'touch', project);
  const facts = { session: event.session,
    request: { edit: event.tool === 'edit', ...(event.event === 'before-prompt' ? { prompt: event.prompt ?? '' } : {}) },
    lifecycle: { started: event.event === 'session-start', contextChanging: event.event === 'before-compact',
      contextRestored: event.event === 'after-compact', operationFinished: event.event === 'after-tool', turnFinished: event.event === 'turn-end' } };
  const plan = decide('context-delivery-plan', { facts, project });
  if (plan.invalidateHint) {
    try { invalidateRoutingHint(root, event.session, event.host); }
    catch { diagnostic('cache','CACHE_UNAVAILABLE'); }
  }
  const intent = plan.intent;
  if (plan.services) result.services = prepareServices(project);
  if (intent) {
    if (event.contextSupported === false) diagnostic('host','HOST_CAPABILITY_UNAVAILABLE');
    const file = relativePath(root, event.file);
    result.routing = { ...route({ intent, files: file ? [file] : [], project, host: event.host }), taskIdentity };
  }
  const session = event.session;
  if (plan.saveCheckpoint) {
    // Store only state/explicit metadata, not prompts, transcripts, tool output or credentials.
    saveCheckpoint(root, session, task ? { taskId: task } : {});
  }
  if (plan.recoverCheckpoint) result.checkpoint = recoverCheckpoint(root, session);
  else if (plan.readCheckpoint) result.checkpoint = { saved: readCheckpoint(root, session) };
  return result;
}

export const evaluateContextDelivery = event => decide('context-delivery', { routing: event.routing, checkpoint: event.checkpoint,
  services: event.services, preserveContinuation: event.event === 'before-compact' });
export const hookFailure = (definition, message) => decide('delivery-failure', {
  enforcementRequired: definition.tool === 'shell' && definition.event === 'before-tool', message });
export const hookDefault = () => decide('delivery-default');
