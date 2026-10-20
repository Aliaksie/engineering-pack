import { stateFile, readState, writeState, lockState, releaseState, telemetryIdentity, readTelemetrySnapshot, clock } from '../../platform/index.mjs';
import { digest } from '../engine-client.mjs';
import { summarizeTelemetry } from './report.mjs';

export const CAPTURE_MAX_BYTES = 65536;
export const CAPTURE_MAX_ENTRIES = 8;
const captureFile = (root, create = false) => stateFile(root, 'telemetry-captures.json', create);
const identifier = value => typeof value === 'string' && /^[a-f0-9]{24}$/.test(value);
const timestamp = value => typeof value === 'string' && /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/.test(value) && Number.isFinite(Date.parse(value));
const { limits: reportLimitNotes, ...template } = summarizeTelemetry([], {
  files: 0, bytes: 0, backupPresent: false, malformedLines: 0, unsupportedRecords: 0, oversizedLines: 0,
  maxFileBytes: 1048576, backupCount: 1, completeHistory: false, concurrentSnapshot: true
}, null);
const maps = {
  'attribution.packages': ['craft-delivery', 'craft-engineering', 'craft-review', 'craft-stories', 'unknown'],
  'attribution.hookInputs': ['complete', 'invalid', 'oversized', 'read-failure', 'unknown']
};

// Retained reports have the same bounded, content-free shape as live reports.
function validReport(value, expected = template, field = '') {
  if (Object.hasOwn(maps, field)) return value && typeof value === 'object' && !Array.isArray(value)
    && Object.entries(value).every(([key, count]) => maps[field].includes(key) && Number.isSafeInteger(count) && count >= 0);
  if (expected === null) return field.startsWith('window.') ? value === null || timestamp(value) : value === null;
  if (typeof expected === 'number') return Number.isSafeInteger(value) && value >= 0 && (field !== 'formatVersion' || value === 1);
  if (typeof expected === 'boolean') return typeof value === 'boolean'
    && (!['storage.completeHistory', 'storage.concurrentSnapshot'].includes(field) || value === expected);
  if (typeof expected === 'string') return field.endsWith('.status') ? ['observed', 'unobserved'].includes(value) : value === expected;
  return value && typeof value === 'object' && !Array.isArray(value)
    && Object.keys(value).length === Object.keys(expected).length
    && Object.keys(expected).every(key => Object.hasOwn(value, key) && validReport(value[key], expected[key], field ? `${field}.${key}` : key));
}
function readCaptures(file) {
  const state = readState(file, CAPTURE_MAX_BYTES) ?? { formatVersion: 1, captures: [] };
  if (state.formatVersion !== 1 || Object.keys(state).length !== 2 || !Array.isArray(state.captures)
    || state.captures.length > CAPTURE_MAX_ENTRIES) throw new Error('invalid telemetry capture state');
  const ids = new Set();
  for (const entry of state.captures) {
    if (!entry || Object.keys(entry).length !== 4 || !identifier(entry.id) || !timestamp(entry.capturedAt)
      || typeof entry.sessionId !== 'string' || !/^[a-f0-9]{32}$/.test(entry.sessionId) || !validReport(entry.report)
      || entry.id !== digest([entry.sessionId, entry.report]).slice(0, 24) || ids.has(entry.id)) throw new Error('invalid retained telemetry capture');
    ids.add(entry.id);
  }
  return state;
}
const limits = Object.freeze({ maxEntries: CAPTURE_MAX_ENTRIES, maxBytes: CAPTURE_MAX_BYTES, automaticEviction: false });

export function captureTelemetry(root, { session } = {}) {
  if (typeof session !== 'string' || !session || session.length > 512) throw new Error('telemetry capture requires a bounded --session identity');
  const sessionId = telemetryIdentity(root, session, false);
  if (!sessionId) throw new Error('telemetry correlation key is unavailable');
  const snapshot = readTelemetrySnapshot(root);
  const { limits: _notes, ...report } = summarizeTelemetry(snapshot.events, snapshot.storage, sessionId);
  if (!report.retainedEvents) throw new Error('no retained session-attributed observations to capture');
  const id = digest([sessionId, report]).slice(0, 24), file = captureFile(root, true);
  const ticket = lockState(file);
  if (!ticket) throw new Error('telemetry capture writer is busy');
  try {
    const state = readCaptures(file);
    const existing = state.captures.find(entry => entry.id === id);
    if (existing) return { file, capture: id, capturedAt: existing.capturedAt, unchanged: true, limits };
    if (state.captures.length >= CAPTURE_MAX_ENTRIES) throw new Error('telemetry capture capacity reached; preserve needed evidence and explicitly forget a capture before adding another');
    const capturedAt = clock().iso;
    state.captures.push({ id, capturedAt, sessionId, report });
    writeState(file, state, CAPTURE_MAX_BYTES);
    return { file, capture: id, capturedAt, unchanged: false, limits };
  } finally { releaseState(ticket); }
}

export function readTelemetryCapture(root, id) {
  if (!identifier(id)) throw new Error('invalid telemetry capture id');
  const entry = readCaptures(captureFile(root)).captures.find(capture => capture.id === id);
  if (!entry) throw new Error('telemetry capture not found');
  return { capture: entry.id, capturedAt: entry.capturedAt, report: { ...entry.report, limits: [...reportLimitNotes] }, limits };
}

export function forgetTelemetryCapture(root, id) {
  if (!identifier(id)) throw new Error('invalid telemetry capture id');
  const file = captureFile(root);
  if (!readCaptures(file).captures.some(entry => entry.id === id)) throw new Error('telemetry capture not found');
  const ticket = lockState(file);
  if (!ticket) throw new Error('telemetry capture writer is busy');
  try {
    const state = readCaptures(file), remaining = state.captures.filter(entry => entry.id !== id);
    if (remaining.length === state.captures.length) throw new Error('telemetry capture not found');
    writeState(file, { formatVersion: 1, captures: remaining }, CAPTURE_MAX_BYTES);
    return { forgotten: id, remaining: remaining.length, limits };
  } finally { releaseState(ticket); }
}
