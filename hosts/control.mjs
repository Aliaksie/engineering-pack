import { createCodexControls, openCodexControls } from './codex/control.mjs';
import { createClaudeControls } from './claude/control.mjs';
import { ACP_HOSTS, createAcpControls, openAcpControls } from './acp/control.mjs';
import { featureSupport } from './capabilities.mjs';
import { workerController } from './workers.mjs';
import { actionController } from '../application/session/actions.mjs';

function target(adapter, options) {
  featureSupport(adapter); // Validate even when no transport is requested.
  if (!['jetbrains', 'agents'].includes(adapter)) return adapter;
  const host = options.underlyingHost;
  if (host !== undefined && !['codex', 'claude', ...Object.keys(ACP_HOSTS)].includes(host)) throw new Error('supported explicit underlying host required');
  return host ?? null;
}
function fallback(adapter, options) {
  return { supported: false, adapter, support: featureSupport(adapter),
    workers: workerController(options.root, options), actions: actionController(options.root, options),
    capabilities: () => ({ workers: false, review: false, compaction: false, reason: 'No native underlying-agent binding was supplied.' }), close() {} };
}
export function createHostControls(adapter, options = {}) {
  const host = target(adapter, options);
  if (!host) return fallback(adapter, options);
  const controls = host === 'codex' ? createCodexControls(options) : host === 'claude' ? createClaudeControls(options) : createAcpControls({ ...options, host });
  return { ...controls, supported: true, adapter, nativeHost: host, support: featureSupport(adapter) };
}
export async function openHostControls(adapter, options = {}) {
  const host = target(adapter, options);
  if (!host) return fallback(adapter, options);
  if (host === 'claude') return createHostControls(adapter, options);
  const controls = host === 'codex' ? await openCodexControls(options) : await openAcpControls({ ...options, host });
  return { ...controls, supported: true, adapter, nativeHost: host, support: featureSupport(adapter) };
}
