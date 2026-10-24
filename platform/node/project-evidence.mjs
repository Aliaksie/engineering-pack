import { nativeArtifact } from './native-artifact.mjs';
import { nativeRequest } from './native-command.mjs';
import path from 'node:path';

// Explicit opt-in only. The installed wrapper never downloads or compiles a binary.
export function projectEvidenceProcess({ enabled = false, root, operation, input = {}, binary, timeoutMs = 120000 } = {}) {
  if (!enabled) return { enabled: false, reason: 'Project evidence is disabled' };
  if (!['version', 'refresh', 'status', 'query', 'backup', 'rebuild'].includes(operation)) throw new Error('unknown project evidence operation');
  if (!Number.isSafeInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > 300000) throw new Error('invalid native deadline');
  const executable = binary ?? nativeArtifact();
  const invoke = (operation, input) => nativeRequest(executable, operation, input, { timeoutMs, subject: 'evidence', interrupted: 'refresh may be partial' });
  const version = invoke('version', {});
  if (version?.schemaVersion !== 1 || typeof version.binaryVersion !== 'string') throw new Error('incompatible native evidence binary');
  if (operation === 'version') return version;
  return invoke(operation, { ...input, root: path.resolve(root ?? '.') });
}
