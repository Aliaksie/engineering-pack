import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { run } from './craft.mjs';
import { routeRequest } from './native-route.mjs';

// Recreate the selected routing request through the public native context command.
export function contextReport(bundles, plan, { budgetBytes } = {}) {
  const options = routeRequest(plan);
  if (!options) throw new Error('expected a native routing plan');
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-context-cli-'));
  try {
    if (options.project) {
      fs.mkdirSync(path.join(root, '.craft'));
      fs.writeFileSync(path.join(root, '.craft/project.json'), JSON.stringify(options.project));
    }
    return run(['context-report', '--root', root, '--intent', options.intent,
      ...(options.files ?? []).flatMap(value => ['--file', value]),
      ...(options.risks ?? []).flatMap(value => ['--risk', value]),
      ...[bundles].flat().flatMap(value => ['--bundle', value]),
      ...(budgetBytes === undefined ? [] : ['--budget-bytes', String(budgetBytes)])]);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
}
