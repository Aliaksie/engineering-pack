import { readTelemetrySnapshot, telemetryIdentity } from '../../platform/index.mjs';
import { telemetryEvent } from './events.mjs';

const features = {
  routing: ['routing.completed'], guidanceSelection: ['guidance.selected'],
  discovery: ['discovery.completed'], lookupCache: ['cache.observed'],
  attempts: ['attempt.recorded'], modelAdvice: ['model.advice'],
  checkpoints: ['checkpoint.saved', 'checkpoint.recovered'],
  context: ['context.observed'], sessionAdvice: ['session.advice'],
  review: ['review.completed'], services: ['integration.prepared', 'integration.planned'],
  nativeHooks: ['host.hook']
};

// Counts describe retained observations, never applicability or semantic success.
export function summarizeTelemetry(records, storage, sessionId) {
  const counts = Object.fromEntries(Object.keys(features).map(name => [name, { status: 'unobserved', events: 0, failures: 0, deferred: 0 }]));
  let retainedEvents = 0, invalidRecords = 0, invocationEvents = 0, sessionEvents = 0;
  let first = null, last = null, reportedLocalDrops = 0, reportedQueueDrops = 0, exportFailures = 0;
  for (const record of records) {
    let event;
    try { event = telemetryEvent(record); } catch { invalidRecords++; continue; }
    if (sessionId !== undefined && (event.sessionId !== sessionId || event.attributes.identityScope !== 'session')) continue;
    retainedEvents++;
    if (event.attributes.identityScope === 'session') sessionEvents++;
    else invocationEvents++;
    if (first === null || event.timestamp < first) first = event.timestamp;
    if (last === null || event.timestamp > last) last = event.timestamp;
    if (event.reasonCode === 'TELEMETRY_EVENTS_DROPPED') reportedLocalDrops += event.attributes.droppedEvents ?? 0;
    if (event.reasonCode === 'EXPORT_QUEUE_FULL') reportedQueueDrops += event.attributes.droppedEvents ?? 0;
    if (event.name === 'telemetry.export' && event.outcome === 'failure') exportFailures++;
    for (const [name, names] of Object.entries(features)) {
      if (!names.includes(event.name)) continue;
      const item = counts[name]; item.status = 'observed'; item.events++;
      if (event.outcome === 'failure') item.failures++;
      if (event.outcome === 'deferred') item.deferred++;
    }
  }
  return {
    formatVersion: 1, scope: sessionId === undefined ? 'retained-project' : 'retained-session',
    retainedEvents, identity: { sessionEvents, invocationOrUnknownEvents: invocationEvents },
    window: { first, last }, storage: { ...storage, invalidRecords },
    loss: { reportedLocalDrops, reportedQueueDrops, exportFailures, totalLostEvents: null },
    features: counts,
    limits: [
      'Retained best-effort observations cannot establish complete history; rotation, deletion, disabled sinks and unreported losses remain unknown.',
      'Storage and invalid-record counts cover the project files, including when events are filtered by session. Reads do not lock concurrent writers.',
      'Unobserved does not mean disabled, unsupported or inapplicable. Observed events do not prove guidance use, acceptance quality or native action execution.',
      'Attempt events are observations, not a sum of cumulative recordedAttempts. No task, model, worker or compaction execution is inferred.'
    ]
  };
}

export function telemetryReport(root, { session } = {}) {
  if (session !== undefined && (typeof session !== 'string' || !session || session.length > 512)) throw new Error('invalid telemetry session');
  const snapshot = readTelemetrySnapshot(root);
  // A report must never create a correlation key just to search retained data.
  const identity = session === undefined ? undefined : telemetryIdentity(root, session, false);
  return summarizeTelemetry(snapshot.events, snapshot.storage, identity);
}
