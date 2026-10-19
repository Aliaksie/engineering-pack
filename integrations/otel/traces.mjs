// OTLP/HTTP JSON mapping only; no provider objects cross the event boundary.
// https://opentelemetry.io/docs/specs/otlp/#json-protobuf-encoding
function attribute(key, value) {
  return { key, value: typeof value === 'boolean' ? { boolValue: value } : typeof value === 'number' ? { doubleValue: value } : { stringValue: value } };
}
export function otlpTraces(events) {
  return { resourceSpans: events.map(event => {
    const end = Date.parse(event.timestamp);
    const span = { traceId: event.traceId ?? event.operationId.replaceAll('-', ''), spanId: event.operationId.replaceAll('-', '').slice(0,16),
      name: event.name, kind: 1,
      ...(event.parentOperationId ? { parentSpanId:event.parentOperationId.replaceAll('-','').slice(0,16) } : {}),
      startTimeUnixNano: (BigInt(Math.max(0, Math.round(end - (event.durationMs ?? 0)))) * 1000000n).toString(),
      endTimeUnixNano: (BigInt(end) * 1000000n).toString(),
      attributes: Object.entries({ ...event.attributes, ...(event.reasonCode ? { 'engineering-pack.reason.code':event.reasonCode } : {}), 'engineering-pack.event.kind':event.kind, 'engineering-pack.session.id': event.sessionId, 'engineering-pack.outcome': event.outcome }).filter(([,v]) => v !== undefined).map(([k,v]) => attribute(k,v)),
      status: { code: event.outcome === 'failure' ? 2 : 0 } };
    return { resource: { attributes: [attribute('service.name','engineering-pack'), ...(event.packVersion ? [attribute('service.version', event.packVersion)] : []), ...(event.host ? [attribute('engineering-pack.host',event.host)] : [])] },
      scopeSpans: [{ scope: { name: 'engineering-pack', version: event.packVersion }, spans: [span] }] };
  }) };
}

// Interpret bounded HTTP observations here, never in portable events or Node I/O.
export function otlpExportOutcome(result) {
  if (result === true) return { ok:true }; // Explicit injected transports may return a boolean.
  const statusCode = result?.statusCode;
  if (!result?.ok) return { ok:false, ...(statusCode ? { statusCode } : {}),
    reasonCode:result?.failure === 'timeout' ? 'EXPORT_TIMEOUT' : result?.failure === 'invalid-response' ? 'EXPORT_INVALID_RESPONSE' : 'EXPORT_FAILED' };
  if (!result.json || typeof result.json !== 'object' || Array.isArray(result.json)) return { ok:false, statusCode, reasonCode:'EXPORT_INVALID_RESPONSE' };
  const partial = result.json.partialSuccess;
  if (partial !== undefined) {
    const count = partial?.rejectedSpans ?? 0;
    if (!['string','number'].includes(typeof count) || !/^\d+$/.test(String(count))) return { ok:false, statusCode, reasonCode:'EXPORT_INVALID_RESPONSE' };
    const rejected = Number(count);
    if (rejected > 0) return { ok:false, statusCode, reasonCode:'EXPORT_PARTIAL_REJECTED',
      ...(Number.isSafeInteger(rejected) && rejected <= 1e9 ? { rejectedEvents:rejected } : {}) };
    if (partial?.errorMessage) return { ok:true, statusCode, reasonCode:'EXPORT_WARNING' };
  }
  return { ok:true, statusCode };
}
