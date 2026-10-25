import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { run } from '../cli/main.mjs';

// One native `route` in a scratch project configured with `project`.
export function route({ intent, files = [], project, risks = [], verificationMode, task, host, delegate } = {}) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-route-')));
  try {
    if (project) {
      fs.mkdirSync(path.join(root, '.craft'));
      fs.writeFileSync(path.join(root, '.craft/project.json'), JSON.stringify(project));
    }
    return run(['route', '--root', root, ...(intent ? ['--intent', intent] : []), ...files.flatMap(file => ['--file', file]),
      ...risks.flatMap(risk => ['--risk', risk]), ...(verificationMode ? ['--verification-mode', verificationMode] : []), ...(task ? ['--task', task] : []),
      ...(host ? ['--host', host] : []), ...(delegate ? ['--delegate'] : [])]);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
}
