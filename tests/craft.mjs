import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

// Test access to the development `craft` (CRAFT_NATIVE_BINARY, set by the test runner)
// through its one-request stdin protocol, which returns structured results and I/O codes.
const repository = fileURLToPath(new URL('../', import.meta.url));
const bundled = ['context-report', 'session', 'session-advice', 'task-step', 'task-offer', 'task-action-check'];

export function nativeRequest(operation, input, { binary = process.env.CRAFT_NATIVE_BINARY, timeoutMs = 120000 } = {}) {
  if (!binary || !path.isAbsolute(binary)) throw new Error('CRAFT_NATIVE_BINARY must name the development craft executable');
  const child = spawnSync(binary, [], { input: JSON.stringify({ formatVersion: 1, operation, input }), encoding: 'utf8',
    timeout: timeoutMs, maxBuffer: 4 * 1024 * 1024, windowsHide: true, shell: false });
  if (child.error) throw child.error;
  if (child.signal) throw new Error(`craft interrupted by ${child.signal}`);
  const response = JSON.parse(child.stdout);
  if (!response.ok) throw Object.assign(new Error(response.error.message), { code: response.error.code });
  return response.result;
}

// One command with its full outcome (`structured`: JSON instead of a compact receipt).
export function nativeCommand(argv, { binary, structured, data, packageName } = {}) {
  return nativeRequest('cli', { argv, structured, package: packageName, ...(data === undefined ? {} : { data }) }, { binary });
}

// The structured result of `craft <command>`, with this checkout as the installed package.
export function run(argv) {
  const [action] = argv;
  const outcome = nativeCommand(bundled.includes(action) && !argv.includes('--bundle') ? [...argv, '--bundle', repository] : argv, { structured: true });
  if (outcome.exitCode) throw Object.assign(new Error(outcome.stderr.trimEnd()), { code: outcome.errorCode ?? undefined });
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
