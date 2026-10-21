import { observe, diagnostic, withTelemetry } from './telemetry/observe.mjs';
import { decide } from './engine-client.mjs';
import { stateFile, readState, lockState, releaseState, writeState, clock } from '../platform/index.mjs';
import { activeTask, readSessionState, taskRepository } from './session/activity.mjs';
import { repositorySnapshot } from './repositories.mjs';

const filename = (root, session, create) => stateFile(root, `${decide('hash', { text: String(session) }).slice(0,24)}.json`, create, session);
function readSavedCheckpoint(root, session) {
  const state = readState(filename(root, session), 65536);
  return decide('checkpoint-validate', { state });
}
function saveCheckpointOperation(root, session, metadata = {}, { expectedSource } = {}) {
  decide('checkpoint-metadata', { metadata });
  const file = filename(root, session, true);
  const ticket = lockState(file);
  if (!ticket) { diagnostic('checkpoint','STATE_BUSY'); throw new Error('checkpoint writer is busy; retry at the next milestone'); }
  try {
    const previous = readCheckpoint(root, session);
    const task = metadata.taskId ?? activeTask(root, session);
    const repositories = metadata.repositories ?? (task ? readSessionState(root, session, task).taskRecord?.repositories : previous?.repositories);
    const repository = repositorySnapshot(root, repositories);
    if (expectedSource !== undefined && decide('task-source', { repository }) !== expectedSource) throw new Error('Source changed after acceptance; continuation was not rebound to newer source');
    const state = decide('checkpoint-state', { previous: previous ?? {}, metadata: { ...metadata, ...(repositories ? { repositories } : {}) }, repository, updated: clock().iso });
    if (decide('checkpoint-unchanged', { previous, state })) return { file, state: previous, unchanged: true };
    writeState(file, state, 65536, true);
    return { file, state, unchanged: false };
  } finally { releaseState(ticket); }
}
export const gitState = (root, session, task) => taskRepository(root, session, task);
function recoverCheckpointOperation(root, session) {
  const saved = readCheckpoint(root, session);
  return decide('checkpoint-compare', { saved, current: repositorySnapshot(root, saved?.repositories) });
}

export const saveCheckpoint = (...args) => withTelemetry(args[0], {}, () => observe('checkpoint.saved', () => saveCheckpointOperation(...args), result => ({ checkpoints: result.unchanged ? 0 : 1 }), { failureCode:'CHECKPOINT_UNAVAILABLE' }));

export const recoverCheckpoint = (...args) => withTelemetry(args[0], {}, () => observe('checkpoint.recovered', () => recoverCheckpointOperation(...args), result => ({ available:Boolean(result.saved), complete:result.changeDetection?.complete }), { failureCode:'CHECKPOINT_UNAVAILABLE' }));

export const readCheckpoint = (...args) => withTelemetry(args[0], {}, () => observe('checkpoint.read', () => readSavedCheckpoint(...args), result => ({ available:Boolean(result) }), { failureCode:'CHECKPOINT_UNAVAILABLE' }));
