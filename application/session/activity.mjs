import { observe } from '../telemetry/observe.mjs';
import { decide, digest } from '../engine-client.mjs';
import { stateFile, readState, lockState, releaseState, writeState, repositoryState, clock, uniqueId } from '../../platform/index.mjs';

function filename(root, session, task, create = false) {
  decide('activity-identity', { session, task });
  return stateFile(root, `session-${digest([session,task]).slice(0,24)}.json`, create);
}
function read(file) {
  return decide('activity-validate', { state: readState(file, 131072) ?? { formatVersion: 1, activity: {}, lookups: {}, attempts: [] } });
}
export const readSessionState = (root, session, task) => read(filename(root,session,task));
export function updateSessionState(root, session, task, update) {
  const file = filename(root,session,task,true);
  const ticket = lockState(file);
  if (!ticket) throw new Error('session state writer is busy');
  try {
    const state = read(file); update(state);
    writeState(file, state, 131072);
    return state;
  } finally { releaseState(ticket); }
}
export const addActivity = (state, values) => Object.assign(state, decide('activity-update', { state, values }));
function recordAttemptOperation(root, session, task, attempt) {
  const receipt = decide('attempt-receipt', { attempt, repository: repositoryState(root), id: uniqueId(), at: clock().iso });
  return updateSessionState(root, session, task, state => Object.assign(state, decide('attempt-append', { state, receipt })));
}

export const attemptStatus = (records, operation) => decide('attempt-status', { records, operation });

export const recordAttempt = (...args) => observe('attempt.recorded', () => recordAttemptOperation(...args), result => ({ attemptNumber: result.attempts.length, reason: 'recorded' }));
