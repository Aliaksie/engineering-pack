import { decide } from '../engine-client.mjs';
import { locate } from './locate.mjs';
import { inventoryIdentity, realRoot } from '../../platform/index.mjs';
import { readSessionState, updateSessionState } from '../session/activity.mjs';

export function discover(root, input = {}, { session, task } = {}) {
  const request = decide('discovery-request', { input });
  const { options, terms, paths } = request;
  if (!terms.length) return decide('discovery-deferred', { request });
  root = realRoot(root);
  let state, identity, diagnostic;
  if (session && task) {
    try {
      state = readSessionState(root, session, task);
      identity = inventoryIdentity(root, paths, options.maxFiles ?? 500);
    } catch (error) { diagnostic = error.message; }
  }
  const { reused, fresh } = decide('lookup-reuse', { terms, paths, identity, state });
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
    } catch (error) { diagnostic = error.message; }
  }
  return decide('discovery-result', { request, scanned, state, identity, reused, fresh, stable: Boolean(stable), diagnostic, enabled: Boolean(session && task) });
}
