// The persisted state layout `craft` writes, as a test oracle: never a runtime reader.
// `<physical root>/.craft/state/<worktree>/<shared|session>/<name>`, where each identity is the
// first 24 hex digits of SHA-256 over the physical root path or the session ID.
import fs from 'node:fs';
import path from 'node:path';
import { createHash, createHmac, randomBytes, randomUUID } from 'node:crypto';

const sha256 = text => createHash('sha256').update(text).digest('hex');
const identity = text => sha256(text).slice(0, 24);

// SHA-256 over the JSON text, as record and receipt identities use it.
export const digest = value => sha256(JSON.stringify(value) ?? '');

export function stateLocation(root, session) {
  const canonical = fs.realpathSync.native(root);
  const worktreeId = identity(canonical), sessionId = session === undefined ? 'shared' : identity(session);
  return { directory: path.join(canonical, '.craft', 'state', worktreeId, sessionId), scope: 'workspace-local', formatVersion: 2,
    worktreeId, sessionId, legacyDirectory: path.join(canonical, '.craft', 'state') };
}

export function stateFile(root, name, create = false, session) {
  const { directory } = stateLocation(root, session);
  if (create) fs.mkdirSync(directory, { recursive: true, mode: 0o700 });
  return path.join(directory, name);
}

export function readJson(file) {
  try { return JSON.parse(fs.readFileSync(file, 'utf8')); }
  catch (error) { if (error.code === 'ENOENT') return null; throw error; }
}

// Schema 1 events of the shared local log, its rotated backup first; other lines are skipped.
export function readTelemetry(root) {
  const file = stateFile(root, 'telemetry.jsonl'), events = [];
  for (const name of [`${file}.1`, file]) {
    if (!fs.existsSync(name)) continue;
    for (const line of fs.readFileSync(name, 'utf8').split('\n')) {
      try { const event = JSON.parse(line); if (event?.schemaVersion === 1) events.push(event); } catch {}
    }
  }
  return events;
}

// The pack version every event records; `package.json` owns it.
export const packVersion = () => readJson(new URL('../package.json', import.meta.url)).version;

// The session pseudonym events carry: HMAC-SHA-256 of the session under the project key.
export function sessionPseudonym(root, session) {
  const { key } = readJson(stateFile(root, 'telemetry-key.json'));
  return createHmac('sha256', key).update(session).digest('hex').slice(0, 32);
}

// One local event as `craft` records it: a session pseudonym (HMAC-SHA-256 under the project
// key) or, without a session, a fresh invocation identity.
export function recordEvent(root, name, { session, outcome = 'success', attributes = {}, reasonCode } = {}) {
  const keyFile = stateFile(root, 'telemetry-key.json', true);
  if (!fs.existsSync(keyFile)) fs.writeFileSync(keyFile, `${JSON.stringify({ key: randomBytes(32).toString('hex') })}\n`, { mode: 0o600 });
  const id = () => randomUUID().replaceAll('-', '');
  const event = { schemaVersion: 1, name, kind: 'observation', timestamp: new Date().toISOString(),
    sessionId: session === undefined ? id() : sessionPseudonym(root, session),
    traceId: id(), operationId: randomUUID(), packVersion: packVersion(),
    attributes: { ...attributes, identityScope: session === undefined ? 'invocation' : 'session' }, outcome,
    ...(reasonCode === undefined ? {} : { reasonCode }) };
  fs.appendFileSync(stateFile(root, 'telemetry.jsonl', true), `${JSON.stringify(event)}\n`);
  return event;
}
