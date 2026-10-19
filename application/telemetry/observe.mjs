import { clock, uniqueId, runtimeVersion, telemetryConfiguration, telemetryIdentity, appendTelemetry, postTelemetry } from '../../platform/index.mjs';
import { otlpTraces } from '../../integrations/otel/traces.mjs';
import { telemetryEvent } from './events.mjs';
import { catalog } from '../routing/catalog.mjs';

let active;
const pending = [];
const noop = { emit() {} };
export function telemetryOptions(value = {}) {
  if (!value || typeof value !== 'object' || Array.isArray(value) || Object.keys(value).some(key => !['local','remote'].includes(key))
    || (value.local !== undefined && typeof value.local !== 'boolean')) throw new Error('invalid telemetry configuration');
  let remote;
  if (value.remote !== undefined) {
    const config = value.remote;
    if (!config || typeof config !== 'object' || Array.isArray(config) || Object.keys(config).some(key => !['enabled','endpoint'].includes(key)) || typeof config.enabled !== 'boolean') throw new Error('invalid remote telemetry');
    if (config.enabled) {
      const url = new URL(config.endpoint);
      if ((url.protocol !== 'https:' && !(url.protocol === 'http:' && ['localhost','127.0.0.1','[::1]'].includes(url.hostname))) || url.username || url.password || url.search || url.hash) throw new Error('invalid telemetry endpoint');
      remote = url.href;
    }
  }
  return { local: value.local !== false, remote };
}
export function createTelemetry(root, { session, host } = {}, ports = {}) {
  const io = { clock, uniqueId, runtimeVersion, telemetryConfiguration, telemetryIdentity, appendTelemetry, ...ports };
  try {
    const options = telemetryOptions(io.telemetryConfiguration(root));
    if (!options.local && !options.remote) return noop;
    const scope = typeof session === 'string' && session.length > 0 && session.length <= 512 ? 'session' : 'invocation';
    let sessionId;
    const version = io.runtimeVersion();
    const identifiers = catalog.entries.flatMap(entry => entry.skills);
    return { identityScope: scope, emit(name, attributes = {}, outcome = 'success', durationMs) {
      try {
        sessionId ??= io.telemetryIdentity(root, scope === 'session' ? session : io.uniqueId());
        const event = telemetryEvent({ name, timestamp: io.clock().iso, sessionId, operationId: io.uniqueId(), packVersion: version, host,
          attributes: { ...attributes, identityScope: scope }, outcome, durationMs }, identifiers);
        if (options.local) { try { io.appendTelemetry(root,event); } catch { /* Advisory storage only. */ } }
        if (options.remote && pending.length < 32) pending.push({ endpoint: options.remote, event });
      } catch { /* Telemetry never changes an operation outcome. */ }
    } };
  } catch { return noop; }
}
export function withTelemetry(root, identity, callback) {
  if (active && !(active.identityScope === 'invocation' && typeof identity?.session === 'string' && identity.session)) return callback();
  const previous = active;
  active = createTelemetry(root,identity);
  try { return callback(); } finally { active = previous; }
}
export function emitTelemetry(name, attributes, outcome, durationMs) { active?.emit(name,attributes,outcome,durationMs); }
export function observe(name, callback, summarize = () => ({})) {
  const started = clock().ms;
  try {
    const result = callback();
    try { emitTelemetry(name,summarize(result),'success',Math.max(0,clock().ms-started)); } catch { /* Projection is advisory. */ }
    return result;
  } catch (error) { emitTelemetry(name,{},'failure',Math.max(0,clock().ms-started)); throw error; }
}
export async function flushTelemetry(transport = postTelemetry) {
  const batch = pending.splice(0,32);
  const groups = new Map();
  for (const {endpoint,event} of batch) { if (!groups.has(endpoint)) groups.set(endpoint,[]); groups.get(endpoint).push(event); }
  await Promise.all([...groups].map(async ([endpoint,events]) => { try { await transport(endpoint,otlpTraces(events)); } catch { /* No retry or operation failure. */ } }));
}
