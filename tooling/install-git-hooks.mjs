import { execFileSync } from 'node:child_process';
import { existsSync, readdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repository = fileURLToPath(new URL('../', import.meta.url));
let configured = false;
try {
  const result = execFileSync('git', ['config', '--get', 'core.hooksPath'], { cwd: repository, encoding: 'utf8' }).trim();
  configured = true;
  if (path.resolve(repository, result) !== path.join(repository, '.githooks')) {
    throw new Error(`Existing core.hooksPath is ${result}; compose its hooks explicitly before installing.`);
  }
} catch (error) {
  if (error.status !== 1) throw error;
}
if (!configured) {
  const hooks = path.resolve(repository, execFileSync('git', ['rev-parse', '--git-path', 'hooks'], { cwd: repository, encoding: 'utf8' }).trim());
  if (existsSync(hooks) && readdirSync(hooks).some(name => !name.endsWith('.sample'))) {
    throw new Error('Existing default Git hooks found; compose their hooks explicitly before installing.');
  }
}
execFileSync('git', ['config', '--local', 'core.hooksPath', '.githooks'], { cwd: repository });
process.stdout.write('Installed repository Git guards (commits/pushes; file edits are not intercepted).\n');
