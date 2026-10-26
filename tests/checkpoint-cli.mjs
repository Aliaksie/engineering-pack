import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { run } from './craft.mjs';

// Exercise checkpoint behavior through the installed command contract.
export function saveCheckpoint(root, session, metadata = {}) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-checkpoint-input-'));
  try {
    const file = path.join(directory, 'metadata.json');
    fs.writeFileSync(file, JSON.stringify(metadata));
    return run(['checkpoint-save', '--root', root, '--session', session, '--data', file]);
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
}
export const recoverCheckpoint = (root, session) => run(['checkpoint-read', '--root', root, '--session', session]);
export const readCheckpoint = (root, session) => recoverCheckpoint(root, session).saved;
// The current repository observation the checkpoint comparison uses.
export const repositoryState = root => run(['checkpoint-read', '--root', root]).current;
