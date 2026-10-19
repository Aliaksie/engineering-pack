import { withTelemetry, emitTelemetry } from '../telemetry/observe.mjs';
import { decide } from '../engine-client.mjs';
import { stateFile, readState, lockState, releaseState, writeState, clock, uniqueId, sourceIdentity } from '../../platform/index.mjs';

const hash = text => decide('hash', { text });
const cacheFile = (root, session, host) => stateFile(root, `routing-${hash(JSON.stringify([host,session])).slice(0,24)}.json`, true);
function read(file) {
  const state = readState(file, 2048);
  return decide('hint-validate', { state });
}
let source;
export function invalidateRoutingHint(root, session, host) {
  const file = `${cacheFile(root,session,host)}.generation`;
  // Independent atomic generation preserves invalidation during an in-flight delivery.
  read(file);
  writeState(file, { formatVersion: 1, key: hash(uniqueId()), emittedAt: clock().ms }, 2048);
  return true;
}
export function reuseRoutingHint(event, result, now = clock().ms) {
  return withTelemetry(event.routingRoot ?? event.cwd ?? '.', { session: event.session, host: event.host }, () => reuseHint(event,result,now));
}
function reuseHint(event, result, now) {
  if (!decide('hint-eligible', { event, result })) return { result };
  try {
    const file = cacheFile(event.routingRoot,event.session,event.host);
    const ticket = lockState(file);
    if (!ticket) return { result };
    try {
      const previous = read(file);
      const generation = read(`${file}.generation`)?.key ?? null;
      source ??= hash(sourceIdentity(['engine/routing/route.mjs','engine/routing/guidance.mjs','engine/routing/catalog.json']));
      const key = hash(JSON.stringify({ sourceIdentity: source, generation, project: event.project, context: result.context }));
      if (decide('hint-valid', { previous, key, now })) {
        emitTelemetry('cache.observed', { reused: true, cacheHits: 1, cacheMisses: 0 });
        const { context, ...quiet } = result;
        releaseState(ticket);
        return { result: quiet, suppressed: true };
      }
      emitTelemetry('cache.observed', { reused: false, cacheHits: 0, cacheMisses: 1 });
      return { result, ticket, state: { formatVersion: 1, key, emittedAt: now } };
    } catch (error) { releaseState(ticket); throw error; }
  } catch (error) { return { result, diagnostic: `Routing hint reuse unavailable: ${error.message}` }; }
}
export function commitRoutingHint(pending, emitted) {
  if (!pending.ticket) return;
  try { if (emitted) { read(pending.ticket.file); writeState(pending.ticket.file, pending.state, 2048); } }
  finally { releaseState(pending.ticket); }
}
