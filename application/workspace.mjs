import { decide, digest } from './engine-client.mjs';
import { readProject } from './project/configuration.mjs';
import { activeTask, readSessionState } from './session/activity.mjs';
import { workspaceOperation, clock } from '../platform/index.mjs';
import { emitTelemetry, diagnostic } from './telemetry/observe.mjs';

// Task, session and host boundaries over the main scope; `workspace-*` commands run in `craft`.
function workspace(root, session, task, action, project) {
  const settings = project ?? readProject(root);
  const policy = settings.workspace ?? decide('effective-settings', { project: settings }).settings.workspace;
  if (!policy.enabled) return { status: 'disabled' };
  const selected = task ?? activeTask(root, session);
  if (action !== 'maintain' && (!session || !selected)) return { status: 'unavailable', reason: 'A verified session and task binding are required.' };
  if (session && selected) decide('activity-identity', { session, task: selected });
  const repositories = session && selected ? readSessionState(root, session, selected).taskRecord?.repositories : undefined;
  const identity = session && selected ? { session: digest(session), task: digest(repositories ? [selected, repositories] : selected), operation: digest('main') } : null;
  const started = clock().ms;
  const result = workspaceOperation(root, identity, policy, action);
  if (action !== 'status' && (result.status === 'available' || result.cleaned?.length || result.retained?.length)) emitTelemetry('workspace.observed', {
    workspaceAction: action, scratchBytes: result.scratchUsage?.bytes ?? 0, evidenceBytes: result.evidence?.bytes ?? 0,
    evidenceCount: result.evidence?.artifacts ?? 0, workspacesCleaned: new Set(result.cleaned?.map(item => item.entry)).size,
    complete: result.retained?.length === 0
  }, result.retained?.length ? 'deferred' : 'success', clock().ms - started);
  return result;
}

// Optional housekeeping must never prevent useful task or host work.
export function workspaceBoundary(root, session, task, action, project) {
  try { return workspace(root, session, task, action, project); }
  catch (error) { diagnostic('state', 'STATE_UNAVAILABLE'); return { status: 'retained', reason: error.message }; }
}
