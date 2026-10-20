import { observe, diagnostic, withTelemetry } from './telemetry/observe.mjs';
import { decide } from './engine-client.mjs';
import { stateFile, readState, lockState, releaseState, writeState, repositoryState, clock } from '../platform/index.mjs';

const filename = (root, session, create) => stateFile(root, `${decide('hash', { text: String(session) }).slice(0,24)}.json`, create);
function readSavedCheckpoint(root, session) {
  const state = readState(filename(root, session), 65536);
  return decide('checkpoint-validate', { state });
}
function saveCheckpointOperation(root, session, metadata = {}) {
  decide('checkpoint-metadata', { metadata });
  const file = filename(root, session, true);
  const ticket = lockState(file);
  if (!ticket) { diagnostic('checkpoint','STATE_BUSY'); throw new Error('checkpoint writer is busy; retry at the next milestone'); }
  try {
    const previous = readCheckpoint(root, session);
    const state = decide('checkpoint-state', { previous: previous ?? {}, metadata, repository: repositoryState(root), updated: clock().iso });
    if (decide('checkpoint-unchanged', { previous, state })) return { file, state: previous, unchanged: true };
    writeState(file, state, 65536, true);
    return { file, state, unchanged: false };
  } finally { releaseState(ticket); }
}
export const gitState = root => repositoryState(root);
function recoverCheckpointOperation(root, session) {
  return decide('checkpoint-compare', { saved: readCheckpoint(root, session), current: repositoryState(root) });
}

export const saveCheckpoint = (...args) => withTelemetry(args[0], {}, () => observe('checkpoint.saved', () => saveCheckpointOperation(...args), result => ({ checkpoints: result.unchanged ? 0 : 1 }), { failureCode:'CHECKPOINT_UNAVAILABLE' }));

export const recoverCheckpoint = (...args) => withTelemetry(args[0], {}, () => observe('checkpoint.recovered', () => recoverCheckpointOperation(...args), result => ({ available:Boolean(result.saved), complete:result.changeDetection?.complete }), { failureCode:'CHECKPOINT_UNAVAILABLE' }));

export const readCheckpoint = (...args) => withTelemetry(args[0], {}, () => observe('checkpoint.read', () => readSavedCheckpoint(...args), result => ({ available:Boolean(result) }), { failureCode:'CHECKPOINT_UNAVAILABLE' }));
