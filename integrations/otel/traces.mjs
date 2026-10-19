// OTLP/HTTP JSON mapping only; no provider objects cross the event boundary.
// https://opentelemetry.io/docs/specs/otlp/#json-protobuf-encoding
function attribute(key, value) {
  return { key, value: typeof value === 'boolean' ? { boolValue: value } : typeof value === 'number' ? { doubleValue: value } : { stringValue: value } };
}
export function otlpTraces(events) {
  return { resourceSpans: events.map(event => {
    const end = Date.parse(event.timestamp);
    const span = { traceId: event.operationId.replaceAll('-', ''), spanId: event.operationId.replaceAll('-', '').slice(0,16),
      name: event.name, kind: 1,
      startTimeUnixNano: (BigInt(Math.max(0, Math.round(end - (event.durationMs ?? 0)))) * 1000000n).toString(),
      endTimeUnixNano: (BigInt(end) * 1000000n).toString(),
      attributes: Object.entries({ ...event.attributes, 'engineering-pack.session.id': event.sessionId, 'engineering-pack.outcome': event.outcome }).filter(([,v]) => v !== undefined).map(([k,v]) => attribute(k,v)),
      status: { code: event.outcome === 'failure' ? 2 : 0 } };
    return { resource: { attributes: [attribute('service.name','engineering-pack'), attribute('service.version', event.packVersion), ...(event.host ? [attribute('engineering-pack.host',event.host)] : [])] },
      scopeSpans: [{ scope: { name: 'engineering-pack', version: event.packVersion }, spans: [span] }] };
  }) };
}
