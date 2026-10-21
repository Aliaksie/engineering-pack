import fs from 'node:fs';
import path from 'node:path';
import http from 'node:http';
import https from 'node:https';
import { randomBytes, createHmac } from 'node:crypto';
import { AsyncLocalStorage } from 'node:async_hooks';
import { performance } from 'node:perf_hooks';
import { stateFile, lockState, releaseState, readState } from './state-files.mjs';

export const TELEMETRY_MAX_BYTES = 1048576;
export const TELEMETRY_BATCH_EVENTS = 32;
const contexts = new AsyncLocalStorage();
const lockWait = new Int32Array(new SharedArrayBuffer(4));
export const telemetryContext = () => contexts.getStore();
export const runTelemetryContext = (context, callback) => contexts.run(context, callback);
export function telemetryRoot(root) {
  try { return fs.realpathSync(root); } catch { return path.resolve(root); }
}
export function telemetryConfiguration(root) {
  const directory = path.resolve(root, '.craft');
  if (fs.lstatSync(directory, { throwIfNoEntry: false })?.isSymbolicLink()) throw new Error('invalid telemetry directory');
  return readState(path.join(directory, 'telemetry.json'), 4096) ?? {};
}
function regular(file, maximum) {
  const stat = fs.lstatSync(file, { throwIfNoEntry: false });
  if (stat && (!stat.isFile() || stat.isSymbolicLink() || stat.size > maximum)) throw new Error('invalid telemetry file');
  return stat;
}
export function telemetryIdentity(root, session, create = true) {
  const file = stateFile(root, 'telemetry-key.json', create);
  let data = readState(file, 256);
  if (!data && !create) return null;
  if (!data) {
    try { fs.writeFileSync(file, `${JSON.stringify({ key: randomBytes(32).toString('hex') })}\n`, { flag: 'wx', mode: 0o600 }); }
    catch (error) { if (error.code !== 'EEXIST') throw error; }
    data = readState(file, 256);
  }
  if (!/^[a-f0-9]{64}$/.test(data?.key)) throw new Error('invalid telemetry key');
  return createHmac('sha256', data.key).update(session).digest('hex').slice(0,32);
}
export function appendTelemetry(root, event) {
  return appendTelemetryBatch(root, [event]);
}
export function appendTelemetryBatch(root, events) {
  if (!Array.isArray(events) || events.length < 1 || events.length > TELEMETRY_BATCH_EVENTS) throw new Error('invalid telemetry batch');
  const text = events.map(event => {
    const line = `${JSON.stringify(event)}\n`;
    if (Buffer.byteLength(line) > 8192) throw new Error('telemetry event too large');
    return line;
  }).join('');
  const file = stateFile(root, 'telemetry.jsonl', true);
  let ticket;
  try {
    // Native packages can write concurrently. Briefly wait for an active writer,
    // but never remove its lock or let stale locks block engineering work.
    const deadline = performance.now() + 25;
    do {
      ticket = lockState(file);
      if (ticket) break;
      const remaining = deadline - performance.now();
      if (remaining <= 0) break;
      Atomics.wait(lockWait, 0, 0, Math.min(2, remaining));
    } while (performance.now() < deadline);
  }
  catch (error) {
    // Windows can deny reopening a lock while another writer deletes it. A
    // denied/busy telemetry lock drops this event; never write without ownership.
    if (['EPERM', 'EACCES', 'EBUSY'].includes(error.code)) return false;
    throw error;
  }
  if (!ticket) return false;
  try {
    const stat = regular(file, TELEMETRY_MAX_BYTES);
    const backup = `${file}.1`;
    regular(backup, TELEMETRY_MAX_BYTES);
    if ((stat?.size ?? 0) + Buffer.byteLength(text) > TELEMETRY_MAX_BYTES) {
      fs.rmSync(backup, { force: true }); fs.renameSync(file, backup);
    }
    const fd = fs.openSync(file, fs.constants.O_WRONLY | fs.constants.O_APPEND | fs.constants.O_CREAT | (fs.constants.O_NOFOLLOW ?? 0), 0o600);
    try { fs.writeFileSync(fd, text); } finally { fs.closeSync(fd); }
    return true;
  } finally { releaseState(ticket); }
}
export function readTelemetry(root) {
  return readTelemetrySnapshot(root).events;
}
export function readTelemetrySnapshot(root) {
  const file = stateFile(root, 'telemetry.jsonl');
  const events = [];
  const storage = { files: 0, bytes: 0, backupPresent: false, malformedLines: 0, unsupportedRecords: 0, oversizedLines: 0,
    maxFileBytes: TELEMETRY_MAX_BYTES, backupCount: 1, completeHistory: false, concurrentSnapshot: true };
  for (const name of [`${file}.1`, file]) {
    const stat = regular(name, TELEMETRY_MAX_BYTES);
    if (!stat) continue;
    storage.files++; storage.bytes += stat.size;
    if (name.endsWith('.1')) storage.backupPresent = true;
    for (const line of fs.readFileSync(name, 'utf8').split('\n')) {
      if (!line) continue;
      if (Buffer.byteLength(line) > 8192) { storage.oversizedLines++; continue; }
      try { const event = JSON.parse(line); if (event?.schemaVersion === 1) events.push(event); else storage.unsupportedRecords++; }
      catch { storage.malformedLines++; }
    }
  }
  return { events, storage };
}

// Transport only: payload construction and policy remain outside the platform.
export async function postTelemetry(endpoint, payload, timeoutMs = 300) {
  const body = JSON.stringify(payload);
  if (Buffer.byteLength(body) > 262144) return { ok:false, failure:'request-too-large' };
  return new Promise(resolve => {
    let finished = false;
    const done = value => { if (!finished) { finished = true; clearTimeout(timer); resolve(value); } };
    const url = new URL(endpoint);
    const transport = url.protocol === 'https:' ? https : http;
    const request = transport.request(url, { method: 'POST', headers: { 'content-type': 'application/json', 'content-length': Buffer.byteLength(body) } }, response => {
      let bytes = 0, chunks = [];
      response.on('data', chunk => {
        bytes += chunk.length;
        if (bytes > 16384) { response.destroy(); done({ ok:false, failure:'response-too-large' }); } else chunks.push(chunk);
      });
      response.on('error', () => done({ ok:false, failure:'transport' }));
      response.on('end', () => {
        try {
          const result = JSON.parse(Buffer.concat(chunks).toString('utf8'));
          done({ ok:response.statusCode === 200, statusCode:response.statusCode, json:result });
        } catch { done({ ok:false, statusCode:response.statusCode, failure:'invalid-response' }); }
      });
    });
    const timer = setTimeout(() => { request.destroy(); done({ ok:false, failure:'timeout' }); }, timeoutMs);
    request.on('error', () => done({ ok:false, failure:'transport' }));
    request.end(body);
  });
}
