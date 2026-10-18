import { catalog } from '../routing/catalog.mjs';
import { bundleRoot, clock } from '../../platform/index.mjs';
import { decide } from '../engine-client.mjs';
import { contextReport } from './context.mjs';
import { readCheckpoint, recoverCheckpoint, gitState } from '../checkpoint.mjs';
import { readSessionState } from './activity.mjs';

// Gather bounded observations; the engine owns identity, freshness and action decisions.
export function sessionStatus(input = {}, project = { formatVersion: 1 }, { root = '.', session, task, bundle, now = clock().ms } = {}) {
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
  return decide('session-status', { input, project, catalog, options: { session: sessionId, task: taskId, now, observations } });
}
