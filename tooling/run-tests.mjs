import { readdirSync } from 'node:fs';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const repository = fileURLToPath(new URL('../', import.meta.url));
const suite = fileURLToPath(new URL('../tests/', import.meta.url));
const selected = readdirSync(suite, { withFileTypes: true })
  .filter(entry => entry.isFile() && entry.name.endsWith('.test.mjs'))
  .map(entry => path.join(suite, entry.name)).sort();
if (!selected.length) throw new Error('The test directory contains no runnable suites');
const runner = spawn(process.execPath, ['--test', ...process.argv.slice(2), ...selected], {
  cwd: repository, stdio: 'inherit', shell: false
});
runner.once('error', error => { process.stderr.write(`Cannot start test runner: ${error.message}\n`); process.exitCode = 1; });
runner.once('exit', (code, signal) => { process.exitCode = code ?? (signal ? 1 : 0); });
