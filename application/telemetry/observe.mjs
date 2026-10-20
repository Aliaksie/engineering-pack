import { clock, uniqueId, runtimeVersion, telemetryConfiguration, telemetryIdentity, appendTelemetry, postTelemetry,
  telemetryContext, runTelemetryContext, telemetryRoot } from '../../platform/index.mjs';
import { otlpTraces, otlpExportOutcome } from '../../integrations/otel/traces.mjs';
import { telemetryEvent } from './events.mjs';
import { catalog } from '../routing/catalog.mjs';

const pending = [];
const identifiers = catalog.entries.flatMap(entry => entry.skills);
const noop = { enabled: false, emit() {}, finish() {}, enrich() {}, trace: () => undefined };
const validSession = value => typeof value === 'string' && value.length > 0 && value.length <= 512;
const validPackage = value => ['craft-delivery','craft-engineering','craft-review','craft-stories'].includes(value);
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
export function createTelemetry(root, { session, host, packageName } = {}, ports = {}) {
  const io = { clock, uniqueId, runtimeVersion, telemetryConfiguration, telemetryIdentity, appendTelemetry, ...ports };
  let config, options, invalidConfig = false, version = null;
  try { config = io.telemetryConfiguration(root); options = telemetryOptions(config); }
  catch { invalidConfig = true; options = { local: config?.local !== false }; }
  if (!options.local && !options.remote) return noop;
  try { version = io.runtimeVersion(); } catch { /* Unknown metadata is explicit. */ }
  if (typeof version !== 'string' || !/^\d+\.\d+\.\d+$/.test(version)) version = null;
  let sessionId, traceId, localDrops = 0, queueDrops = 0, initialDiagnostic = invalidConfig, identityUnavailable = false, initialIdentityDiagnostic = false, metadataDiagnostic = version === null;
  const trace = () => traceId ??= io.uniqueId().replaceAll('-','');
  const correlation = () => {
    if (sessionId) return sessionId;
    try { sessionId = io.telemetryIdentity(root,validSession(session) ? session : io.uniqueId()); }
    catch { sessionId = io.uniqueId().replaceAll('-',''); identityUnavailable = true; initialIdentityDiagnostic = true; }
    return sessionId;
  };
  const build = (name, attributes, outcome, durationMs, context = {}, reasonCode, kind) => telemetryEvent({
    name, timestamp: io.clock().iso, sessionId: correlation(),
    operationId: context.operationId ?? io.uniqueId(), traceId: context.traceId ?? trace(), parentOperationId: context.parentOperationId,
    packVersion: version, host, attributes: { ...attributes, ...(validPackage(packageName) ? { package: packageName } : {}), identityScope: validSession(session) && !identityUnavailable ? 'session' : 'invocation' },
    outcome, durationMs, reasonCode, kind
  }, identifiers);
  const write = event => {
    try { return io.appendTelemetry(root,event) !== false; } catch { return false; }
  };
  // These diagnostics bypass the export queue and never diagnose their own failure.
  const localDiagnostic = (reasonCode, attributes = {}, context = {}) => {
    if (!options.local) return false;
    try { return write(build('diagnostic',{ area:'telemetry', ...attributes },'failure',undefined,context,reasonCode)); } catch { return false; }
  };
  const reportQueueDrops = context => {
    if (queueDrops && localDiagnostic('EXPORT_QUEUE_FULL', { droppedEvents:queueDrops }, context)) queueDrops = 0;
  };
  const handle = {
    enabled: true, trace,
    enrich(identity) {
      if (!validSession(session) && validSession(identity.session)) { session = identity.session; sessionId = undefined; identityUnavailable = false; initialIdentityDiagnostic = false; }
      host ??= identity.host;
      packageName ??= identity.packageName;
    },
    emit(name, attributes = {}, outcome = 'success', durationMs, context = {}, reasonCode, kind, localOnly = false) {
      try {
        const event = build(name,attributes,outcome,durationMs,context,reasonCode,kind);
        const diagnosticContext = { traceId:event.traceId, parentOperationId:event.operationId };
        if (initialDiagnostic) { initialDiagnostic = false; localDiagnostic('TELEMETRY_CONFIG_INVALID',{},diagnosticContext); }
        if (metadataDiagnostic) { metadataDiagnostic = false; localDiagnostic('TELEMETRY_METADATA_UNAVAILABLE',{},diagnosticContext); }
        if (initialIdentityDiagnostic) { initialIdentityDiagnostic = false; localDiagnostic('TELEMETRY_IDENTITY_UNAVAILABLE',{},diagnosticContext); }
        if (options.local) {
          if (!write(event)) localDrops = Math.min(1e9, localDrops + 1);
          else if (localDrops) {
            if (localDiagnostic('TELEMETRY_EVENTS_DROPPED',{ droppedEvents:localDrops },diagnosticContext)) localDrops = 0;
          }
        }
        if (options.remote && !localOnly) {
          if (pending.length < 32) pending.push({ endpoint:options.remote, event, handle });
          else {
            queueDrops = Math.min(1e9, queueDrops + 1);
            // Keep a completed root when possible, so bounded export does not
            // discard a command's root while retaining only its children.
            const leaf = !event.parentOperationId && event.kind === 'operation' ? pending.findLastIndex(entry =>
              entry.handle === handle && entry.event.traceId === event.traceId
              && !pending.some(child => child.event.parentOperationId === entry.event.operationId)) : -1;
            if (leaf >= 0) {
              const [dropped] = pending.splice(leaf,1);
              pending.push({ endpoint:options.remote, event, handle });
              reportQueueDrops({ traceId:dropped.event.traceId, parentOperationId:dropped.event.operationId });
            } else reportQueueDrops(diagnosticContext);
          }
        }
      } catch { localDiagnostic('TELEMETRY_EVENT_INVALID',{},context); }
    },
    finish(context = {}) {
      if (localDrops && localDiagnostic('TELEMETRY_EVENTS_DROPPED',{ droppedEvents:localDrops },context)) localDrops = 0;
      reportQueueDrops(context);
    }
  };
  return handle;
}
export function withTelemetry(root, identity = {}, callback) {
  identity = { session:validSession(identity?.session) ? identity.session : undefined, host:identity?.host,
    packageName: validPackage(identity?.packageName) ? identity.packageName : undefined };
  const previous = telemetryContext();
  let normalized;
  try { normalized = telemetryRoot(root); } catch { return callback(); }
  const sameSession = identity.session === undefined || identity.session === previous?.identity.session || !validSession(previous?.identity.session);
  const sameHost = identity.host === undefined || previous?.identity.host === undefined || identity.host === previous.identity.host;
  const samePackage = identity.packageName === undefined || previous?.identity.packageName === undefined || identity.packageName === previous.identity.packageName;
  if (previous?.root === normalized && sameSession && sameHost && samePackage) {
    previous.handle.enrich(identity);
    previous.identity.session ??= identity.session; previous.identity.host ??= identity.host;
    previous.identity.packageName ??= identity.packageName;
    return callback();
  }
  if (previous?.root === normalized && sameSession && sameHost) {
    identity.session ??= previous.identity.session;
    identity.host ??= previous.identity.host;
  }
  let handle;
  try { handle = createTelemetry(normalized,identity); } catch { handle = noop; }
  const frame = { root:normalized, identity:{...identity}, handle };
  return runTelemetryContext(frame, () => {
    const finish = () => { try { handle.finish(); } catch { /* Advisory only. */ } };
    try {
      const result = callback();
      if (result instanceof Promise) return result.finally(finish);
      finish(); return result;
    } catch (error) { finish(); throw error; }
  });
}
function eventContext(frame) {
  return { traceId:frame.handle.trace(), ...(frame.operation ? { parentOperationId:frame.operation.id } : {}) };
}
export function emitTelemetry(name, attributes, outcome, durationMs, reasonCode) {
  const frame = telemetryContext();
  if (!frame?.handle.enabled) return;
  try { frame.handle.emit(name,attributes,outcome,durationMs,eventContext(frame),reasonCode); } catch { /* Advisory only. */ }
}
export function diagnostic(area, reasonCode, attributes = {}) {
  emitTelemetry('diagnostic',{ ...attributes, area },'failure',undefined,reasonCode);
}
function failureCode(error, fallback) {
  const codes = { EPERM:'ACCESS_DENIED', EACCES:'ACCESS_DENIED', ENOSPC:'STORAGE_FULL', ENOENT:'NOT_FOUND', EBUSY:'STATE_BUSY', EEXIST:'STATE_BUSY' };
  return codes[error?.code] ?? fallback ?? 'OPERATION_FAILED';
}
export function observe(name, callback, summarize = () => ({}), options = {}) {
  const parent = telemetryContext();
  if (!parent?.handle.enabled || (options.reuse && parent.operation?.name === name)) return callback();
  let operation, started;
  try { operation = { id:uniqueId(), name }; started = clock().ms; } catch { return callback(); }
  const frame = { ...parent, operation };
  const complete = (result, error, failed = false) => {
    try {
      const context = { traceId:parent.handle.trace(), operationId:operation.id,
        ...(parent.operation ? { parentOperationId:parent.operation.id } : {}) };
      const outcome = failed ? 'failure' : options.outcome?.(result) ?? 'success';
      parent.handle.emit(name,failed ? {} : summarize(result),outcome,Math.max(0,clock().ms-started),context,
        failed ? failureCode(error,options.failureCode) : outcome === 'failure' ? options.failureCode : undefined,'operation');
    } catch { /* Projection, clock and sink failure cannot affect work. */ }
  };
  return runTelemetryContext(frame, () => {
    try {
      const result = callback();
      if (result instanceof Promise) return result.then(value => { complete(value); return value; },error => { complete(undefined,error,true); throw error; });
      complete(result); return result;
    } catch (error) { complete(undefined,error,true); throw error; }
  });
}
export async function flushTelemetry(transport = postTelemetry) {
  const batch = pending.splice(0,32), groups = new Map();
  for (const entry of batch) { if (!groups.has(entry.endpoint)) groups.set(entry.endpoint,[]); groups.get(entry.endpoint).push(entry); }
  await Promise.all([...groups].map(async ([endpoint,entries]) => {
    const started = clock().ms;
    let result;
    try { result = otlpExportOutcome(await transport(endpoint,otlpTraces(entries.map(entry => entry.event)))); }
    catch { result = { ok:false, reasonCode:'EXPORT_FAILED' }; }
    const handles = new Set(entries.map(entry => entry.handle));
    for (const handle of handles) {
      const own = entries.filter(entry => entry.handle === handle), event = own.at(-1).event;
      handle.emit('telemetry.export',{ area:'telemetry', batchSize:own.length, ...(result.ok ? { exportedEvents:own.length } : {}), ...(result.statusCode ? { statusCode:result.statusCode } : {}),
        ...(handles.size === 1 && result.rejectedEvents !== undefined ? { rejectedEvents:result.rejectedEvents } : {}) },result.ok ? 'success' : 'failure',
      Math.max(0,clock().ms-started),{ traceId:event.traceId, parentOperationId:event.operationId },result.reasonCode,'operation',true);
      handle.finish();
    }
  }));
}
