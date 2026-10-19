import fs from 'node:fs';
import path from 'node:path';
import http from 'node:http';
import https from 'node:https';
import { randomBytes, createHmac } from 'node:crypto';
import { stateFile, lockState, releaseState, readState } from './state-files.mjs';

export const TELEMETRY_MAX_BYTES = 1048576;
export function telemetryConfiguration(root) {
  const directory = path.resolve(root, '.capabilities');
  if (fs.lstatSync(directory, { throwIfNoEntry: false })?.isSymbolicLink()) throw new Error('invalid telemetry directory');
  return readState(path.join(directory, 'telemetry.json'), 4096) ?? {};
}
function regular(file, maximum) {
  const stat = fs.lstatSync(file, { throwIfNoEntry: false });
  if (stat && (!stat.isFile() || stat.isSymbolicLink() || stat.size > maximum)) throw new Error('invalid telemetry file');
  return stat;
}
export function telemetryIdentity(root, session) {
  const file = stateFile(root, 'telemetry-key.json', true);
  let data = readState(file, 256);
  if (!data) {
    try { fs.writeFileSync(file, `${JSON.stringify({ key: randomBytes(32).toString('hex') })}\n`, { flag: 'wx', mode: 0o600 }); }
    catch (error) { if (error.code !== 'EEXIST') throw error; }
    data = readState(file, 256);
  }
  if (!/^[a-f0-9]{64}$/.test(data?.key)) throw new Error('invalid telemetry key');
  return createHmac('sha256', data.key).update(session).digest('hex').slice(0,32);
}
export function appendTelemetry(root, event) {
  const text = `${JSON.stringify(event)}\n`;
  if (Buffer.byteLength(text) > 8192) throw new Error('telemetry event too large');
  const file = stateFile(root, 'telemetry.jsonl', true);
  const ticket = lockState(file);
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
  const file = stateFile(root, 'telemetry.jsonl');
  const events = [];
  for (const name of [`${file}.1`, file]) {
    if (!regular(name, TELEMETRY_MAX_BYTES)) continue;
    for (const line of fs.readFileSync(name, 'utf8').split('\n')) {
      if (Buffer.byteLength(line) > 8192) continue;
      try { const event = JSON.parse(line); if (event.schemaVersion === 1) events.push(event); } catch { /* Skip incomplete/malformed lines. */ }
    }
  }
  return events;
}

// Transport only: payload construction and policy remain outside the platform.
export async function postTelemetry(endpoint, payload, timeoutMs = 300) {
  const body = JSON.stringify(payload);
  if (Buffer.byteLength(body) > 262144) return false;
  return new Promise(resolve => {
    let finished = false;
    const done = value => { if (!finished) { finished = true; clearTimeout(timer); resolve(value); } };
    const url = new URL(endpoint);
    const transport = url.protocol === 'https:' ? https : http;
    const request = transport.request(url, { method: 'POST', headers: { 'content-type': 'application/json', 'content-length': Buffer.byteLength(body) } }, response => {
      let bytes = 0, chunks = [];
      response.on('data', chunk => {
        bytes += chunk.length;
        if (bytes > 16384) { response.destroy(); done(false); } else chunks.push(chunk);
      });
      response.on('error', () => done(false));
      response.on('end', () => {
        try {
          const result = JSON.parse(Buffer.concat(chunks).toString('utf8'));
          done(response.statusCode === 200 && !result.partialSuccess?.rejectedSpans && !result.partialSuccess?.errorMessage);
        } catch { done(false); }
      });
    });
    const timer = setTimeout(() => { request.destroy(); done(false); }, timeoutMs);
    request.on('error', () => done(false));
    request.end(body);
  });
}
