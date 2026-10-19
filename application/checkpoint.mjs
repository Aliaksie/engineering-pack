import { observe } from './telemetry/observe.mjs';
import { decide } from './engine-client.mjs';
import { stateFile, readState, lockState, releaseState, writeState, repositoryState, clock } from '../platform/index.mjs';

const filename = (root, session, create) => stateFile(root, `${decide('hash', { text: String(session) }).slice(0,24)}.json`, create);
export function readCheckpoint(root, session) {
  const state = readState(filename(root, session), 65536);
  return decide('checkpoint-validate', { state });
}
function saveCheckpointOperation(root, session, metadata = {}) {
  decide('checkpoint-metadata', { metadata });
  const file = filename(root, session, true);
  const ticket = lockState(file);
  if (!ticket) throw new Error('checkpoint writer is busy; retry at the next milestone');
  try {
    const state = decide('checkpoint-state', { previous: readCheckpoint(root, session) ?? {}, metadata, repository: repositoryState(root), updated: clock().iso });
    writeState(file, state, 65536, true);
    return { file, state };
  } finally { releaseState(ticket); }
}
export const gitState = root => repositoryState(root);
function recoverCheckpointOperation(root, session) {
  return decide('checkpoint-compare', { saved: readCheckpoint(root, session), current: repositoryState(root) });
}

export const saveCheckpoint = (...args) => observe('checkpoint.saved', () => saveCheckpointOperation(...args), () => ({ checkpoints: 1 }));

export const recoverCheckpoint = (...args) => observe('checkpoint.recovered', () => recoverCheckpointOperation(...args), () => ({}));
