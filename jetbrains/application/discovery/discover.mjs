import { observe, emitTelemetry, withTelemetry, diagnostic as diagnosticError } from '../telemetry/observe.mjs';
import { decide } from '../engine-client.mjs';
import { locate } from './locate.mjs';
import { inventoryIdentity, realRoot } from '../../platform/index.mjs';
import { readSessionState, updateSessionState } from '../session/activity.mjs';

function discoverOperation(root, input = {}, { session, task } = {}) {
  const request = decide('discovery-request', { input });
  const { options, terms, paths } = request;
  if (!terms.length) return decide('discovery-deferred', { request });
  root = realRoot(root);
  let state, identity, diagnostic;
  if (session && task) {
    try {
      state = readSessionState(root, session, task);
      identity = inventoryIdentity(root, paths, options.maxFiles ?? 500);
    } catch (error) { diagnosticError('state','STATE_UNAVAILABLE'); diagnostic = error.message; }
  }
  const { reused, fresh } = observe('cache.lookup', () => decide('lookup-reuse', { terms, paths, identity, state }), result => ({ available:Boolean(identity), ...(identity ? { cacheHits:result.reused.length, cacheMisses:result.fresh.length } : {}) }));
  emitTelemetry('cache.observed', identity ? { cacheHits: reused.length, cacheMisses: fresh.length, reused: reused.length > 0, reason: 'observed' } : { reason: 'unavailable' });
  const scanned = fresh.length ? locate(root, { ...options, terms: fresh }) : {
    inventory: 'validated-reuse', terms, paths, filesScanned: 0, bytesRead: 0,
    matches: [], skipped: {}, truncated: false, truncation: [],
    limitations: ['Reused literal navigation evidence; not semantic definitions or proof of absence.']
  };
  // Recheck versions after the scan: a concurrent edit must not certify a reusable packet.
  const stable = identity && inventoryIdentity(root, paths, options.maxFiles ?? 500) === identity;
  if (session && task) {
    try {
      updateSessionState(root, session, task, current => {
        Object.assign(current, decide('discovery-state', { current, input: { request, state, identity, reused, fresh, stable, scanned } }));
      });
    } catch (error) { diagnosticError('state','STATE_UNAVAILABLE'); diagnostic = error.message; }
  }
  return decide('discovery-result', { request, scanned, state, identity, reused, fresh, stable: Boolean(stable), diagnostic, enabled: Boolean(session && task) });
}

export const discover = (...args) => withTelemetry(args[0], { session:args[2]?.session }, () => observe('discovery.completed', () => discoverOperation(...args), result => ({ batchSize: result.discovery?.fresh, evidenceCount: result.matches?.length, reason: result.terms?.length === 0 ? 'deferred' : 'observed' }), { failureCode:'SEARCH_FAILED' }));
