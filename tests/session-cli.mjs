import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { run } from './craft.mjs';

export function sessionStatus(input = {}, _project, { root = '.', session, task, bundle } = {}) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-session-input-'));
  try {
    const file = path.join(directory, 'observations.json');
    fs.writeFileSync(file, JSON.stringify(input));
    return run(['session', '--root', root, '--data', file,
      ...(session ? ['--session', session] : []), ...(task ? ['--task', task] : []),
      ...[bundle ?? []].flat().flatMap(value => ['--bundle', value])]);
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
}
