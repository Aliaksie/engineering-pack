import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

// Test access to the development `craft` (CRAFT_NATIVE_BINARY, set by the test runner): its
// command line, and the one-request stdin protocol for operations without a command.
const repository = fileURLToPath(new URL('../', import.meta.url));
const bundled = ['context-report', 'session', 'session-advice', 'task-step', 'task-offer', 'task-action-check'];

function developmentBinary(binary = process.env.CRAFT_NATIVE_BINARY) {
  if (!binary || !path.isAbsolute(binary)) throw new Error('CRAFT_NATIVE_BINARY must name the development craft executable');
  return binary;
}

export function nativeRequest(operation, input, { binary, timeoutMs = 120000 } = {}) {
  const child = spawnSync(developmentBinary(binary), [], { input: JSON.stringify({ formatVersion: 1, operation, input }), encoding: 'utf8',
    timeout: timeoutMs, maxBuffer: 4 * 1024 * 1024, windowsHide: true, shell: false });
  if (child.error) throw child.error;
  if (child.signal) throw new Error(`craft interrupted by ${child.signal}`);
  const response = JSON.parse(child.stdout);
  if (!response.ok) throw Object.assign(new Error(response.error.message), { code: response.error.code });
  return response.result;
}

// `craft <argv>` with its full outcome (`structured`: `--json` instead of a compact receipt).
export function nativeCommand(argv, { binary, structured } = {}) {
  const child = spawnSync(developmentBinary(binary), structured ? [...argv, '--json'] : argv, { encoding: 'utf8',
    timeout: 120000, maxBuffer: 4 * 1024 * 1024, windowsHide: true, shell: false });
  if (child.error) throw child.error;
  if (child.signal) throw new Error(`craft interrupted by ${child.signal}`);
  return { exitCode: child.status, stdout: child.stdout, stderr: child.stderr };
}

// The structured result of `craft <command>`, with this checkout as the installed package.
export function run(argv) {
  const [action] = argv;
  const outcome = nativeCommand(bundled.includes(action) && !argv.includes('--bundle') ? [...argv, '--bundle', repository] : argv, { structured: true });
  if (outcome.exitCode) throw new Error(outcome.stderr.trimEnd());
  return JSON.parse(outcome.stdout);
}

// One bounded search without session state.
export function nativeLocate(root, request, repositories) {
  return nativeRequest('locate', { root, request, ...(repositories ? { repositories } : {}) });
}

// `<package>/bin/craft <args>` as a person or skill runs it from an installed package.
export function launch(packageRoot, args, options = {}) {
  const spawnOptions = { encoding: 'utf8', ...options };
  if (process.platform !== 'win32') return spawnSync(path.join(packageRoot, 'bin', 'craft'), args, spawnOptions);
  // Node starts .cmd files only through a shell, which takes the command line as written.
  const line = [path.join(packageRoot, 'bin', 'craft.cmd'), ...args].map(arg => `"${arg}"`).join(' ');
  return spawnSync(line, { ...spawnOptions, shell: true });
}
