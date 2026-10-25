import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { run } from '../cli/main.mjs';

const requests = new WeakMap();
export const routeRequest = plan => requests.get(plan);

// One native `route` in a scratch project configured with `project`.
export function route(options = {}) {
  const { intent, files = [], project, risks = [], verificationMode, task, host, delegate } = options;
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-route-')));
  try {
    if (project) {
      fs.mkdirSync(path.join(root, '.craft'));
      fs.writeFileSync(path.join(root, '.craft/project.json'), JSON.stringify(project));
    }
    const plan = run(['route', '--root', root, ...(intent ? ['--intent', intent] : []), ...files.flatMap(file => ['--file', file]),
      ...risks.flatMap(risk => ['--risk', risk]), ...(verificationMode ? ['--verification-mode', verificationMode] : []), ...(task ? ['--task', task] : []),
      ...(host ? ['--host', host] : []), ...(delegate ? ['--delegate'] : [])]);
    requests.set(plan, options);
    return plan;
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
}
