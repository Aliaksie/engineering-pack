import fs from 'node:fs';
import path from 'node:path';
import { createHash, randomUUID } from 'node:crypto';

const hash = (value) => createHash('sha256').update(value).digest('hex');
const MAX_AGE_MS = 15 * 60 * 1000;
let sourceIdentity;
const identity = () => sourceIdentity ??= hash(['./router.mjs', './catalog.mjs', './catalog.json']
  .map((file) => fs.readFileSync(new URL(file, import.meta.url), 'utf8')).join('\n'));
function cacheFile(root, session, host) {
  let directory = path.resolve(root);
  for (const part of ['.capabilities', 'state']) {
    directory = path.join(directory, part);
    const stat = fs.lstatSync(directory, { throwIfNoEntry: false });
    if (stat?.isSymbolicLink()) throw new Error('routing hint state may not traverse symlinks');
    fs.mkdirSync(directory, { recursive: true });
  }
  return path.join(directory, `routing-${hash(JSON.stringify([host, session])).slice(0, 24)}.json`);
}
function read(file) {
  const stat = fs.lstatSync(file, { throwIfNoEntry: false });
  if (!stat) return null;
  if (stat.isSymbolicLink() || !stat.isFile() || stat.size > 2048) throw new Error('invalid routing hint state');
  const state = JSON.parse(fs.readFileSync(file, 'utf8'));
  if (state.formatVersion !== 1 || !(state.key === null || /^[a-f0-9]{64}$/.test(state.key)) || !Number.isFinite(state.emittedAt)) throw new Error('invalid routing hint state');
  return state;
}
function lock(file) {
  try { return fs.openSync(`${file}.lock`, 'wx', 0o600); }
  catch (error) { if (error.code === 'EEXIST') return null; throw error; }
}
function release(ticket) {
  fs.closeSync(ticket.fd);
  fs.unlinkSync(`${ticket.file}.lock`);
}
function write(ticket, state) {
  const temporary = path.join(path.dirname(ticket.file), `.${randomUUID()}.tmp`);
  try {
    // Validate an existing target again before replacement, including dangling symlinks.
    read(ticket.file);
    fs.writeFileSync(temporary, `${JSON.stringify(state)}\n`, { flag: 'wx', mode: 0o600 });
    fs.renameSync(temporary, ticket.file);
  } finally { if (fs.existsSync(temporary)) fs.unlinkSync(temporary); }
}
export function invalidateRoutingHint(root, session, host) {
  const file = `${cacheFile(root, session, host)}.generation`;
  // Independent atomic generation avoids losing a compaction invalidation while an older
  // hint is being emitted. Its subsequent commit retains the old generation in its key.
  write({ file }, { formatVersion: 1, key: hash(randomUUID()), emittedAt: Date.now() });
  return true;
}

// Reuse only emitted routing text. This cache holds hashes/timestamps, never prompts,
// retrieved evidence, verification results or permissions. Busy/unavailable state must emit.
export function reuseRoutingHint(event, result, now = Date.now()) {
  const pending = { result };
  if (!event.routing || event.project?.routing?.dedupeHints !== true || !event.session
    || event.contextSupported !== true || !result.context) return pending;
  try {
    const file = cacheFile(event.routingRoot, event.session, event.host), fd = lock(file);
    if (fd === null) return pending;
    const ticket = { file, fd };
    try {
      const previous = read(file);
      const generation = read(`${file}.generation`)?.key ?? null;
      const key = hash(JSON.stringify({ sourceIdentity: identity(), generation, project: event.project, context: result.context }));
      if (previous?.key === key && now >= previous.emittedAt && now - previous.emittedAt < MAX_AGE_MS) {
        const { context, ...quiet } = result;
        release(ticket);
        return { result: quiet, suppressed: true };
      }
      return { result, ticket, state: { formatVersion: 1, key, emittedAt: now } };
    } catch (error) { release(ticket); throw error; }
  } catch (error) { return { result, diagnostic: `Routing hint reuse unavailable: ${error.message}` }; }
}
export function commitRoutingHint(pending, emitted) {
  if (!pending.ticket) return;
  try { if (emitted) write(pending.ticket, pending.state); }
  finally { release(pending.ticket); }
}
