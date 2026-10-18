import { normalizeScope as locateScope, byteLength } from '../contracts/paths.mjs';
import { digest } from '../cache/identity.mjs';
import { addActivity } from '../session/activity.mjs';
export function discoveryRequest(input) {
  const allowed = ['terms', 'paths', 'maxFiles', 'maxMatches', 'maxBytes', 'maxOutputBytes', 'requests'];
  if (!input || typeof input !== 'object' || Array.isArray(input) || Object.keys(input).some(key => !allowed.includes(key))) throw new Error('invalid bounded locator request');
  const { maxOutputBytes = 32768, requests, ...options } = input;
  if (!Number.isSafeInteger(maxOutputBytes) || maxOutputBytes < 256 || maxOutputBytes > 65536) throw new Error('maxOutputBytes must be 256..65536');
  let terms = options.terms;
  const deferred = [];
  if (requests !== undefined) {
    if (terms !== undefined || !Array.isArray(requests) || requests.length < 1 || requests.length > 32) throw new Error('use terms or 1..32 locator requests');
    const ids = new Set(); terms = [];
    for (const request of requests) {
      if (!request || typeof request !== 'object' || Array.isArray(request)
        || Object.keys(request).some(key => !['id', 'term', 'dependsOn'].includes(key))
        || typeof request.id !== 'string' || !/^[a-z0-9-]{1,64}$/.test(request.id) || ids.has(request.id)
        || typeof request.term !== 'string' || !request.term || request.term.length > 256 || /[\r\n\0]/.test(request.term)
        || (request.dependsOn !== undefined && (!Array.isArray(request.dependsOn) || request.dependsOn.some(id => typeof id !== 'string')))) throw new Error('invalid locator-only request; side effects are unsupported');
      ids.add(request.id);
      if (request.dependsOn?.length) deferred.push({ id: request.id, reason: 'dependent lookup requires a later reasoning stage' });
      else terms.push(request.term);
    }
    for (const request of requests) for (const dependency of request.dependsOn ?? []) if (!ids.has(dependency) || dependency === request.id) throw new Error('invalid lookup dependency');
  }
  // Let the existing locator validate terms/scopes/bounds before any reuse or state write.
  if (!Array.isArray(terms) || (!terms.length && requests === undefined) || terms.length > 32 || terms.some(term => typeof term !== 'string' || !term || term.length > 256 || /[\r\n\0]/.test(term))) throw new Error('invalid locator terms');
  terms = [...new Set(terms)];
  if (options.paths !== undefined && !Array.isArray(options.paths)) throw new Error('paths must be an array');
  const paths = [...new Set((options.paths ?? []).map(locateScope))];
  options.paths = paths;
  for (const [key, ceiling] of [['maxFiles', 10000], ['maxMatches', 1000], ['maxBytes', 16777216]]) {
    if (options[key] !== undefined && (!Number.isSafeInteger(options[key]) || options[key] < 1 || options[key] > ceiling)) throw new Error(`invalid ${key}`);
  }
  return { options, terms, paths, deferred, maxOutputBytes };
}
export function discoveryResult({ request, scanned, state, identity, reused, fresh, stable, diagnostic, enabled }) {
  const { terms, paths, deferred, options, maxOutputBytes } = request;
  const key = term => digest([term, paths]);
  const matches = [], truncation = new Set(scanned.truncation);
  let bytes = 0;
  const append = match => {
    const previous = matches.find(entry => entry.path === match.path && entry.line === match.line && entry.text === match.text);
    if (previous) {
      previous.termIndexes = [...new Set([...previous.termIndexes, ...match.termIndexes])].sort((a, b) => a - b);
      return;
    }
    const size = byteLength(JSON.stringify(match));
    if (matches.length >= (options.maxMatches ?? 80)) { truncation.add('maxMatches'); return; }
    if (bytes + size > maxOutputBytes - 256) { truncation.add('maxOutputBytes'); return; }
    bytes += size; matches.push(match);
  };
  for (const term of reused) for (const match of state.lookups[key(term)].matches) append({ ...match, termIndexes: [terms.indexOf(term)] });
  for (const match of scanned.matches) append({ ...match, termIndexes: match.termIndexes.map(index => terms.indexOf(fresh[index])) });
  const result = { ...scanned, terms, matches, deferred, truncated: truncation.size > 0, truncation: [...truncation],
    discovery: { requested: terms.length + deferred.length, reused: reused.length, fresh: fresh.length, batches: fresh.length ? 1 : 0 },
    reuse: { enabled, identity: identity ?? null, validated: Boolean(stable), diagnostic: diagnostic ?? null,
      invalidation: 'session/task, HEAD, scoped file inventory/versions and root manifests/config; unavailable evidence always scans fresh' },
    bounds: { ...scanned.bounds, maxOutputBytes } };
  while (byteLength(JSON.stringify(result)) > maxOutputBytes && result.matches.length) {
    result.matches.pop(); result.truncated = true;
    if (!result.truncation.includes('maxOutputBytes')) result.truncation.push('maxOutputBytes');
  }
  if (byteLength(JSON.stringify(result)) > maxOutputBytes) throw new Error('output metadata exceeds maxOutputBytes; split into smaller evidence requests');
  return result;
}
export function discoveryState(current, { request, state, identity, reused, fresh, stable, scanned }) {
  const { terms, paths } = request;
  const key = term => digest([term, paths]);
  const requestKey = digest([identity, terms, paths]);
  current.lastDiscovery = { key: requestKey, repetitions: identity && fresh.length === 0 && current.lastDiscovery?.key === requestKey ? current.lastDiscovery.repetitions + 1 : 0 };
  addActivity(current, { discoveryRequests: terms.length, repeatedLookups: terms.filter(term => state?.lookups[key(term)]).length,
    reuseHits: reused.length, freshLookups: fresh.length, batchedOperations: fresh.length ? 1 : 0 });
  if (stable && !scanned.truncated && !Object.values(scanned.skipped).some(Boolean)) {
    for (const term of fresh) current.lookups[key(term)] = { identity, matches: scanned.matches
      .filter(match => match.termIndexes.includes(fresh.indexOf(term))).map(({ termIndexes, ...match }) => match) };
    const entries = Object.entries(current.lookups).slice(-32);
    current.lookups = Object.fromEntries(entries);
    // Keep packets small; dropping old packets never suppresses fresh lookup.
    while (byteLength(JSON.stringify(current.lookups)) > 65536) delete current.lookups[Object.keys(current.lookups)[0]];
  }
  return current;
}
export function deferredDiscovery(request) {
  const { paths, deferred, maxOutputBytes } = request;
  const result = { terms: [], paths, matches: [], deferred, discovery: { requested: deferred.length, reused: 0, fresh: 0, batches: 0 }, truncated: false, bounds: { maxOutputBytes } };
  if (byteLength(JSON.stringify(result)) > maxOutputBytes) throw new Error('output metadata exceeds maxOutputBytes; split into smaller evidence requests');
  return result;
}
