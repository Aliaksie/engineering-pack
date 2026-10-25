// Test fixture access to native session records; never included in runtime exports.
import { observe, diagnostic, withTelemetry } from '../application/telemetry/observe.mjs';
import { decide, digest } from '../application/engine-client.mjs';
import { stateFile, readState, lockState, releaseState, writeState } from '../platform/index.mjs';
import { repositorySnapshot } from '../application/repositories.mjs';

function filename(root, session, task, create = false) {
  decide('activity-identity', { session, task });
  return stateFile(root, `session-${digest([session,task]).slice(0,24)}.json`, create, session);
}
function read(file) {
  return decide('activity-validate', { state: readState(file, 131072) ?? { formatVersion: 1, activity: {}, lookups: {}, attempts: [] } });
}
export const readSessionState = (root, session, task) => withTelemetry(root, { session }, () => observe('state.read', () => read(filename(root,session,task)), () => ({ area:'session' }), { failureCode:'STATE_UNAVAILABLE' }));
function updateState(root, session, task, update) {
  const file = filename(root,session,task,true);
  const ticket = lockState(file);
  if (!ticket) { diagnostic('state','STATE_BUSY'); throw new Error('session state writer is busy'); }
  try {
    const state = read(file); update(state);
    writeState(file, state, 131072);
    return state;
  } finally { releaseState(ticket); }
}
export const addActivity = (state, values) => Object.assign(state, decide('activity-update', { state, values }));
export const attemptStatus = (records, operation) => decide('attempt-status', { records, operation });

export const updateSessionState = (...args) => withTelemetry(args[0], { session:args[1] }, () => observe('state.updated', () => updateState(...args), () => ({ area:'session' }), { failureCode:'STATE_UNAVAILABLE' }));

export function bindingFile(root, session, create = false) {
  decide('activity-identity', { session, task: 'active-binding' });
  return stateFile(root, `active-${digest(session).slice(0, 24)}.json`, create, session);
}
export function activeTask(root, session) {
  if (!session) return undefined;
  const binding = decide('task-binding', { state: readState(bindingFile(root, session), 16384) });
  if (!binding) return undefined;
  return decide('task-binding', { state: binding, repository: repositorySnapshot(root, binding.repositories, { checkoutOnly: true }) })?.task;
}
