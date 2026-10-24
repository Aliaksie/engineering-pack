import path from 'node:path';
import { nativeArtifact } from './native-artifact.mjs';
import { spawnSync } from 'node:child_process';

// Explicit opt-in only. The installed wrapper never downloads or compiles a binary.
export function projectEvidenceProcess({ enabled = false, root, operation, input = {}, binary, timeoutMs = 120000 } = {}) {
  if (!enabled) return { enabled: false, reason: 'Project evidence is disabled' };
  if (!['version', 'refresh', 'status', 'query', 'backup', 'rebuild'].includes(operation)) throw new Error('unknown project evidence operation');
  if (!Number.isSafeInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > 300000) throw new Error('invalid native deadline');
  const executable = binary ?? nativeArtifact();
  if (!path.isAbsolute(executable)) throw new Error('native binary path must be absolute');
  const invoke = (operation, input) => {
    const request = JSON.stringify({ formatVersion: 1, operation, input });
    if (Buffer.byteLength(request) > 3 * 1024 * 1024) throw new Error('native input exceeds bound');
    const child = spawnSync(executable, [], { input: request, encoding: 'utf8', timeout: timeoutMs, maxBuffer: 4 * 1024 * 1024, windowsHide: true, shell: false });
    if (child.error) throw Object.assign(new Error(`Native evidence unavailable: ${child.error.message}`), { code: 'NATIVE_UNAVAILABLE' });
    if (child.signal) throw Object.assign(new Error('Native evidence interrupted; refresh may be partial'), { code: 'NATIVE_INTERRUPTED' });
    let response;
    try { response = JSON.parse(child.stdout); } catch { throw new Error('invalid native evidence response'); }
    if (response?.formatVersion !== 1 || typeof response.ok !== 'boolean') throw new Error('incompatible native evidence protocol');
    if (!response.ok) throw Object.assign(new Error(response.error?.message ?? 'Native evidence failed'), { code: response.error?.code ?? 'NATIVE_FAILED' });
    if (child.status !== 0) throw new Error('native evidence exited unsuccessfully');
    return response.result;
  };
  const version = invoke('version', {});
  if (version?.schemaVersion !== 1 || typeof version.binaryVersion !== 'string') throw new Error('incompatible native evidence binary');
  if (operation === 'version') return version;
  return invoke(operation, { ...input, root: path.resolve(root ?? '.') });
}
