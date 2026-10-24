import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { nativeArtifact } from './native-artifact.mjs';

// One bounded JSON request per native process; `subject` names the caller in failures.
export function nativeRequest(executable, operation, input, { timeoutMs = 120000, subject = 'runtime', interrupted = 'the command outcome is unknown' } = {}) {
  if (!path.isAbsolute(executable)) throw new Error('native binary path must be absolute');
  const request = JSON.stringify({ formatVersion: 1, operation, input });
  if (Buffer.byteLength(request) > 3 * 1024 * 1024) throw new Error('native input exceeds bound');
  const child = spawnSync(executable, [], { input: request, encoding: 'utf8', timeout: timeoutMs, maxBuffer: 4 * 1024 * 1024, windowsHide: true, shell: false });
  if (child.error) throw Object.assign(new Error(`Native ${subject} unavailable: ${child.error.message}`), { code: 'NATIVE_UNAVAILABLE' });
  if (child.signal) throw Object.assign(new Error(`Native ${subject} interrupted; ${interrupted}`), { code: 'NATIVE_INTERRUPTED' });
  let response;
  try { response = JSON.parse(child.stdout); } catch { throw new Error(`invalid native ${subject} response`); }
  if (response?.formatVersion !== 1 || typeof response.ok !== 'boolean') throw new Error(`incompatible native ${subject} protocol`);
  if (!response.ok) throw Object.assign(new Error(response.error?.message ?? `Native ${subject} failed`), { code: response.error?.code ?? 'NATIVE_FAILED' });
  if (child.status !== 0) throw new Error(`native ${subject} exited unsuccessfully`);
  return response.result;
}

// A migrated CLI invocation. CRAFT_NATIVE_BINARY selects an absolute development build.
export function nativeCommand(argv, { binary = process.env.CRAFT_NATIVE_BINARY, telemetry, discovery } = {}) {
  const result = nativeRequest(binary || nativeArtifact(), 'cli', { argv, telemetry, discovery });
  if (!Number.isInteger(result?.exitCode)) throw new Error('incompatible native runtime protocol');
  return result;
}

// Bounded search for legacy session/discovery callers until their W2 cutover.
export function nativeLocate(root, request, repositories) {
  return nativeRequest(process.env.CRAFT_NATIVE_BINARY || nativeArtifact(), 'locate', { root, request, ...(repositories ? { repositories } : {}) });
}
