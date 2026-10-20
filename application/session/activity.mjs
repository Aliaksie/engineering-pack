import { readProject } from '../project/configuration.mjs';
import { observe, diagnostic, withTelemetry } from '../telemetry/observe.mjs';
import { decide, digest } from '../engine-client.mjs';
import { stateFile, readState, lockState, releaseState, writeState, repositoryState, clock, uniqueId } from '../../platform/index.mjs';

function filename(root, session, task, create = false) {
  decide('activity-identity', { session, task });
  return stateFile(root, `session-${digest([session,task]).slice(0,24)}.json`, create);
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
function recordAttemptOperation(root, session, task, attempt, { preferencesFile } = {}) {
  const project = readProject(root, { preferencesFile });
  const profileLimit = project.execution?.profiles?.[attempt.profile]?.maxAttempts;
  const boundedAttempt = { maxAttempts: profileLimit ?? project.efficiency.attempts.maxAttempts, ...attempt };
  const receipt = decide('attempt-receipt', { attempt: boundedAttempt, repository: repositoryState(root), id: uniqueId(), at: clock().iso });
  return updateSessionState(root, session, task, state => Object.assign(state, decide('attempt-append', { state, receipt })));
}

export const attemptStatus = (records, operation) => decide('attempt-status', { records, operation });

export const recordAttempt = (...args) => withTelemetry(args[0], { session:args[1] }, () => observe('attempt.recorded', () => recordAttemptOperation(...args), result => ({ recordedAttempts: result.activity.operationsAttempted, reason: 'recorded' }), { failureCode:'VALIDATION_FAILED' }));

export const updateSessionState = (...args) => withTelemetry(args[0], { session:args[1] }, () => observe('state.updated', () => updateState(...args), () => ({ area:'session' }), { failureCode:'STATE_UNAVAILABLE' }));
