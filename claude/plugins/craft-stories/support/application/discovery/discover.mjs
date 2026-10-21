import { observe, emitTelemetry, withTelemetry, diagnostic as diagnosticError } from '../telemetry/observe.mjs';
import { readProject } from '../project/configuration.mjs';
import { decide, digest } from '../engine-client.mjs';
import { locate } from './locate.mjs';
import { inventoryIdentity, realRoot, readState } from '../../platform/index.mjs';
import { readSessionState, updateSessionState, activeTask, bindingFile } from '../session/activity.mjs';

function discoverOperation(root, input = {}, { session, task, preferencesFile } = {}) {
  if (session && !task) {
    try { task = activeTask(root, session); }
    catch { diagnosticError('state', 'STATE_UNAVAILABLE'); }
  }
  let repositories;
  if (session && task) {
    try { repositories = readSessionState(root, session, task).taskRecord?.repositories; }
    catch {
      diagnosticError('state', 'STATE_UNAVAILABLE');
      // A damaged activity ledger cannot validate reuse, but a separately verified
      // binding can still keep a fresh read within the declared repository scope.
      try {
        const binding = decide('task-binding', { state: readState(bindingFile(root, session), 16384) });
        if (binding?.task === task) repositories = binding.repositories;
      } catch { diagnosticError('state', 'STATE_UNAVAILABLE'); }
    }
  }
  if (repositories) {
    const paths = input.paths?.length ? input.paths : repositories;
    if (paths.some(file => !repositories.some(scope => scope === '.' || file === scope || file.startsWith(`${scope}/`)))) throw new Error('lookup paths must belong to a selected repository');
    input = { ...input, paths };
  }
  const request = decide('discovery-request', { input });
  const { options, terms, paths } = request;
  if (!terms.length) return decide('discovery-deferred', { request });
  root = realRoot(root);
  const project = readProject(root, { preferencesFile });
  const policyIdentity = digest(project);
  const enabled = Boolean(session && task && project.efficiency.lookup.enabled && project.efficiency.lookup.reuse);
  let state, identity, inventory, diagnostic;
  if (enabled) {
    try {
      state = readSessionState(root, session, task);
      inventory = repositories ? null : inventoryIdentity(root, paths, options.maxFiles ?? 500);
      identity = inventory ? digest([inventory, policyIdentity]) : undefined;
    } catch (error) { diagnosticError('state','STATE_UNAVAILABLE'); diagnostic = error.message; }
  }
  const { reused, fresh } = observe('cache.lookup', () => decide('lookup-reuse', { terms, paths, identity, state }), result => ({ available:Boolean(identity), ...(identity ? { cacheHits:result.reused.length, cacheMisses:result.fresh.length } : {}) }));
  emitTelemetry('cache.observed', identity ? { cacheHits: reused.length, cacheMisses: fresh.length, reused: reused.length > 0, reason: 'observed' } : { reason: 'unavailable' });
  const scanned = fresh.length ? locate(root, { ...options, terms: fresh }, { repositories: repositories ? decide('repository-scopes', { repositories }) : undefined }) : {
    inventory: 'validated-reuse', terms, paths, filesScanned: 0, bytesRead: 0,
    matches: [], skipped: {}, truncated: false, truncation: [],
    limitations: ['Reused literal navigation evidence; not semantic definitions or proof of absence.']
  };
  // Recheck versions after the scan: a concurrent edit must not certify a reusable packet.
  const stable = identity && inventoryIdentity(root, paths, options.maxFiles ?? 500) === inventory && digest(readProject(root, { preferencesFile })) === policyIdentity;
  if (enabled) {
    try {
      updateSessionState(root, session, task, current => {
        Object.assign(current, decide('discovery-state', { current, input: { request, state, identity, reused, fresh, stable, scanned } }));
      });
    } catch (error) { diagnosticError('state','STATE_UNAVAILABLE'); diagnostic = error.message; }
  }
  return decide('discovery-result', { request, scanned, state, identity, reused, fresh, stable: Boolean(stable), diagnostic, enabled });
}

export const discover = (...args) => withTelemetry(args[0], { session:args[2]?.session }, () => observe('discovery.completed', () => discoverOperation(...args), result => ({ batchSize: result.discovery?.fresh, evidenceCount: result.matches?.length, reason: result.terms?.length === 0 ? 'deferred' : 'observed' }), { failureCode:'SEARCH_FAILED' }));
