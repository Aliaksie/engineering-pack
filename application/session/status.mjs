import { observe, emitTelemetry, withTelemetry } from '../telemetry/observe.mjs';
import { catalog } from '../routing/catalog.mjs';
import { bundleRoot, clock, runtimeVersion } from '../../platform/index.mjs';
import { decide } from '../engine-client.mjs';
import { contextReport } from './context.mjs';
import { readCheckpoint, recoverCheckpoint, gitState } from '../checkpoint.mjs';
import { readSessionState } from './activity.mjs';

// Gather bounded observations; the engine owns identity, freshness and action decisions.
function sessionStatusOperation(input = {}, project = { formatVersion: 1 }, { root = '.', session, task, bundle, now = clock().ms } = {}) {
  if (session && input.sessionId && session !== input.sessionId) throw new Error('session input identity mismatch');
  if (task && input.task?.id && task !== input.task.id) throw new Error('task input identity mismatch');
  const sessionId = session ?? input.sessionId, taskId = task ?? input.task?.id;
  input = { ...input, ...(sessionId ? { sessionId } : {}), task: { ...input.task, ...(taskId ? { id: taskId } : {}) } };
  const observations = {};
  if (sessionId && taskId) {
    try { observations.state = readSessionState(root, sessionId, taskId); }
    catch (error) { observations.stateError = error.message; }
  }
  if (sessionId) {
    try { observations.saved = readCheckpoint(root, sessionId); }
    catch (error) { observations.checkpointError = error.message; }
  }
  const needed = decide('session-observation-plan', { input, project, catalog, state: observations.state, saved: observations.saved });
  if (needed.recover) {
    try { observations.recovered = recoverCheckpoint(root, sessionId); observations.repository = observations.recovered.current; }
    catch (error) { observations.checkpointError = error.message; }
  }
  if (needed.repository && !observations.repository) observations.repository = gitState(root);
  if (needed.packError) observations.packError = needed.packError;
  if (needed.plan) {
    try {
      observations.plan = needed.plan;
      observations.pack = contextReport(bundle ?? bundleRoot(), needed.plan);
    } catch (error) { observations.packError = error.message; }
  }
  const result = decide('session-status', { input, project, catalog, options: { session: sessionId, task: taskId, now, observations } });
  emitTelemetry('session.advice', { decision: result.recommendation?.action, reason: result.activity.maxAttempts != null && result.activity.attempts >= result.activity.maxAttempts ? 'budget-exhausted' : 'observed' });
  emitTelemetry('host.observed', { occupancyKnown: result.usage?.status === 'observed', reason: result.usage?.status === 'observed' ? 'observed' : 'unavailable' });
  let version = null;
  try { version = runtimeVersion(); } catch { /* Missing supporting metadata cannot break the view. */ }
  return { ...result, packVersion: version };
}

export const sessionStatus = (...args) => withTelemetry(args[2]?.root ?? '.', { session: args[2]?.session ?? args[0]?.sessionId, host: args[0]?.host?.name }, () => observe('session.observed', () => sessionStatusOperation(...args), result => ({ decision: result.recommendation?.action, pressure: result.pressure, occupancyKnown: result.usage?.status === 'observed' })));
