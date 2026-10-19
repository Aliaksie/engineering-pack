import { withTelemetry, flushTelemetry, emitTelemetry } from '../telemetry/observe.mjs';
export { flushTelemetry };
import { projectRoot, relativePath } from '../../platform/index.mjs';
import { readProject } from '../project/configuration.mjs';
import { route } from '../routing/select.mjs';
import { prepareServices } from '../services.mjs';
import { decide } from '../engine-client.mjs';
import { readCheckpoint, recoverCheckpoint, saveCheckpoint } from '../checkpoint.mjs';
import { invalidateRoutingHint } from './hints.mjs';
export { reuseRoutingHint, commitRoutingHint } from './hints.mjs';

export function prepareEvent(event) {
  return withTelemetry(projectRoot(event.cwd), { session: event.session, host: event.host }, () => prepare(event));
}
function prepare(event) {
  emitTelemetry('host.observed', { lifecycle: event.event });
  const root = projectRoot(event.cwd);
  const project = readProject(root);
  const result = { ...event, project, routingRoot: root };
  const facts = { session: event.session,
    request: { edit: event.tool === 'edit', ...(event.event === 'before-prompt' ? { prompt: event.prompt ?? '' } : {}) },
    lifecycle: { started: event.event === 'session-start', contextChanging: event.event === 'before-compact',
      contextRestored: event.event === 'after-compact', operationFinished: event.event === 'after-tool', turnFinished: event.event === 'turn-end' } };
  const plan = decide('context-delivery-plan', { facts, project });
  if (plan.invalidateHint) {
    try { invalidateRoutingHint(root, event.session, event.host); }
    catch { /* Reuse is advisory; unavailable state never authorizes or blocks work. */ }
  }
  const intent = plan.intent;
  if (plan.services) result.services = prepareServices(project);
  if (intent) {
    const file = relativePath(root, event.file);
    result.routing = route({ intent, files: file ? [file] : [], project, host: event.host });
  }
  const session = event.session;
  if (plan.saveCheckpoint) {
    // Store only state/explicit metadata, not prompts, transcripts, tool output or credentials.
    saveCheckpoint(root, session);
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
