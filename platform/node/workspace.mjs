import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { stateFile, readState, writeState, lockState, releaseState } from './state-files.mjs';

const hash = value => createHash('sha256').update(value).digest('hex');
const digest = value => typeof value === 'string' && /^[a-f0-9]{64}$/.test(value);
const name = value => typeof value === 'string' && /^[a-zA-Z0-9][a-zA-Z0-9._-]{0,127}$/.test(value) && value !== '..';
const live = pid => {
  try { process.kill(pid, 0); return true; }
  catch (error) { return error.code !== 'ESRCH'; }
};
function directory(file, create = false) {
  let stat = fs.lstatSync(file, { throwIfNoEntry: false });
  if (!stat && create) {
    try { fs.mkdirSync(file, { mode: 0o700 }); }
    catch (error) { if (error.code !== 'EEXIST') throw error; }
    stat = fs.lstatSync(file);
  }
  if (stat && (!stat.isDirectory() || stat.isSymbolicLink())) throw new Error('workspace directory must not be a symlink');
  return Boolean(stat);
}
function contents(root) {
  const result = { bytes: 0, files: 0 };
  if (!directory(root)) return result;
  let entries = 0;
  const walk = (file, depth = 0) => {
    if (depth > 16 || ++entries > 4096) throw new Error('workspace inspection limit reached');
    const stat = fs.lstatSync(file);
    if (stat.isSymbolicLink()) throw new Error('workspace contains a symlink; cleanup refused');
    if (stat.isFile()) { result.bytes += stat.size; result.files++; }
    else if (stat.isDirectory()) {
      const dir = fs.opendirSync(file);
      try { let child; while ((child = dir.readSync())) walk(path.join(file, child.name), depth + 1); }
      finally { dir.closeSync(); }
    } else throw new Error('workspace contains an unsupported file');
  };
  walk(root);
  return result;
}
function manifest(file, identity, key) {
  const value = readState(file, 32768);
  if (!value || value.owner !== 'craft' || value.formatVersion !== 1 || value.project !== identity
    || !digest(value.session) || !digest(value.task) || hash(`${value.session}:${value.task}`) !== key
    || !Number.isSafeInteger(value.updated) || value.updated < 0 || typeof value.closed !== 'boolean'
    || !Array.isArray(value.evidence) || value.evidence.length > 64
    || !value.operations || typeof value.operations !== 'object' || Array.isArray(value.operations)
    || Object.keys(value.operations).length > 64
    || Object.entries(value.operations).some(([id, op]) => !digest(id) || !Number.isSafeInteger(op.pid) || op.pid < 1 || typeof op.closed !== 'boolean')
    || value.evidence.some(item => !name(item.file) || !Number.isSafeInteger(item.at) || !Number.isSafeInteger(item.bytes) || item.bytes < 0)) throw new Error('workspace ownership metadata is invalid');
  return value;
}
const active = value => Object.values(value.operations).some(op => !op.closed && live(op.pid));
function removeTree(file) {
  contents(file); // Inspect before removal; cooperative writers share the workspace lock.
  if (directory(file)) fs.rmSync(file, { recursive: true });
  if (fs.existsSync(file)) throw new Error('workspace cleanup could not be verified');
}

function workspaceLock(anchor) {
  let ticket = lockState(anchor);
  if (!ticket) {
    const file = `${anchor}.lock`;
    let stat, owner;
    try { stat = fs.lstatSync(file); owner = readState(file, 1024); }
    catch (error) {
      // The holder may be publishing or releasing its lock. Defer this attempt.
      if (error.code === 'ENOENT' || error instanceof SyntaxError) return null;
      throw error;
    }
    if (owner?.owner !== 'craft-workspace-lock' || owner.formatVersion !== 1 || !Number.isSafeInteger(owner.pid) || owner.pid < 1 || live(owner.pid)) return null;
    const current = fs.lstatSync(file);
    if (stat.ino !== current.ino || stat.dev !== current.dev) return null;
    fs.unlinkSync(file);
    ticket = lockState(anchor);
  }
  if (ticket) {
    try { fs.writeFileSync(ticket.fd, JSON.stringify({ owner: 'craft-workspace-lock', formatVersion: 1, pid: process.pid })); }
    catch (error) { releaseState(ticket); throw error; }
  }
  return ticket;
}

// All cooperating writers and cleanup serialize on one project-private lock. A live
// owner is retained even when old; age alone never establishes abandonment.
export function workspaceOperation(root, identity, policy, action, input = {}, now = Date.now()) {
  if (!['open', 'write', 'promote', 'release', 'complete', 'maintain', 'touch', 'status'].includes(action)) throw new Error('unknown workspace operation');
  if (identity && ![identity.session, identity.task, identity.operation].every(digest)) throw new Error('verified workspace identity required');
  const create = ['open', 'write'].includes(action);
  if (!policy.enabled) return { status: 'disabled' };
  const anchor = stateFile(root, 'workspace-guard.json', create);
  const sessions = path.join(path.dirname(anchor), 'sessions');
  if (!directory(sessions, create)) return { status: 'absent', scratch: 'absent', evidence: { artifacts: 0, bytes: 0 } };
  const ticket = workspaceLock(anchor);
  if (!ticket) return { status: 'retained', reason: 'workspace is in use' };
  try {
    const project = hash(fs.realpathSync.native(root)), cleaned = [], retained = [];
    const key = identity ? hash(`${identity.session}:${identity.task}`) : null;
    const entries = [];
    const dir = fs.opendirSync(sessions);
    try {
      let entry;
      while ((entry = dir.readSync())) {
        if (entries.length >= 64) throw new Error('workspace session inspection limit reached');
        entries.push(entry.name);
      }
    } finally { dir.closeSync(); }
    const owned = [];
    for (const entry of entries.sort()) {
      try {
        if (!digest(entry)) throw new Error('unknown workspace entry');
        const folder = path.join(sessions, entry);
        directory(folder);
        const value = manifest(path.join(folder, 'owner.json'), project, entry);
        owned.push({ entry, folder, value });
      } catch (error) { retained.push({ entry, reason: error.message }); }
    }
    const removed = () => cleaned.filter(item => item.removed).length, room = create && identity && !entries.includes(key) ? 1 : 0; // unverified entries occupy capacity
    if (['maintain', 'open', 'write'].includes(action)) {
      for (const item of owned.sort((a, b) => a.value.updated - b.value.updated)) {
        if (item.entry === key || active(item.value)) continue;
        try {
          const age = now - item.value.updated;
          if ((item.value.closed || age >= policy.scratch.staleAfterHours * 3600000) && directory(path.join(item.folder, 'scratch'))) {
            removeTree(path.join(item.folder, 'scratch'));
            cleaned.push({ entry: item.entry, scratch: 'cleaned' });
          }
          if (age >= policy.evidence.retentionDays * 86400000 || entries.length - removed() + room > policy.maxSessions) {
            if (fs.readdirSync(item.folder).some(name => !['owner.json', 'scratch', 'evidence'].includes(name))) throw new Error('unknown workspace files; cleanup refused');
            removeTree(item.folder); cleaned.push({ entry: item.entry, removed: true });
          }
        } catch (error) { retained.push({ entry: item.entry, reason: error.message }); }
      }
    }
    if (!identity) return { status: 'inspected', cleaned, retained };
    if (![identity.session, identity.task, identity.operation].every(digest)) throw new Error('verified workspace identity required');
    const folder = path.join(sessions, key);
    if (!directory(folder)) {
      if (!create) return { status: 'absent', cleaned, retained };
      if (entries.length - removed() >= policy.maxSessions) throw new Error('workspace session capacity reached; active or unverified workspaces were retained');
      directory(folder, true);
      writeState(path.join(folder, 'owner.json'), { owner: 'craft', formatVersion: 1, project, session: identity.session, task: identity.task,
        updated: now, closed: false, operations: {}, evidence: [] }, 32768, true);
    }
    const owner = path.join(folder, 'owner.json'), value = manifest(owner, project, key);
    if (value.session !== identity.session || value.task !== identity.task) throw new Error('workspace identity mismatch');
    const scratch = path.join(folder, 'scratch'), evidence = path.join(folder, 'evidence');
    const scope = path.join(scratch, identity.operation);
    const currentOwner = value.operations[identity.operation];
    if (['open', 'write', 'promote', 'release'].includes(action) && currentOwner && !currentOwner.closed
      && currentOwner.pid !== process.pid && live(currentOwner.pid)) throw new Error('workspace operation belongs to another active owner');
    if (['open', 'write'].includes(action)) {
      if (!Object.hasOwn(value.operations, identity.operation) && Object.keys(value.operations).length >= 64) throw new Error('workspace operation capacity reached');
      directory(scratch, true); directory(scope, true);
      value.operations[identity.operation] = { pid: process.pid, closed: false };
      value.closed = false;
    }
    if (action === 'write') {
      if (!name(input.name) || input.name.length > 100 || typeof input.content !== 'string') throw new Error('workspace write requires a safe filename of at most 100 characters and text');
      const usage = contents(scratch), bytes = Buffer.byteLength(input.content);
      if (usage.files >= policy.scratch.maxFiles || usage.bytes + bytes > policy.scratch.maxBytes) throw new Error('workspace scratch budget exceeded');
      fs.writeFileSync(path.join(scope, input.name), input.content, { flag: 'wx', mode: 0o600 });
    }
    if (action === 'promote') {
      if (!policy.evidence.enabled) return { status: 'disabled', reason: 'evidence retention is disabled' };
      if (!name(input.name) || input.name.length > 100 || !name(input.purpose) || value.evidence.length >= policy.evidence.maxArtifacts) throw new Error('invalid promotion or evidence count exceeded');
      directory(scratch); directory(scope); directory(evidence, true);
      const source = path.join(scope, input.name), stat = fs.lstatSync(source);
      const usage = contents(evidence);
      if (!stat.isFile() || stat.isSymbolicLink() || stat.nlink !== 1 || usage.files >= policy.evidence.maxArtifacts || usage.bytes + stat.size > policy.evidence.maxBytes) throw new Error('unsafe or oversized evidence');
      const target = `${identity.operation.slice(0, 16)}-${input.name}`;
      if (fs.existsSync(path.join(evidence, target))) throw new Error('evidence already exists');
      const fd = fs.openSync(source, fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW ?? 0));
      try {
        const opened = fs.fstatSync(fd);
        if (opened.ino !== stat.ino || opened.dev !== stat.dev) throw new Error('promotion source changed');
        const buffer = Buffer.alloc(stat.size + 1), count = fs.readSync(fd, buffer, 0, buffer.length, 0);
        if (count !== stat.size) throw new Error('promotion source changed size');
        fs.writeFileSync(path.join(evidence, target), buffer.subarray(0, count), { flag: 'wx', mode: 0o600 });
      } finally { fs.closeSync(fd); }
      value.evidence.push({ file: target, at: now, bytes: stat.size, purpose: input.purpose, operation: identity.operation, sourceAtPromotion: input.source ?? null });
    }
    if (action === 'release') {
      const operation = value.operations[identity.operation];
      if (operation && operation.pid !== process.pid && live(operation.pid)) throw new Error('workspace operation belongs to another active owner');
      if (operation) operation.closed = true;
    }
    if (action === 'complete') {
      value.closed = true;
      for (const op of Object.values(value.operations)) if (op.pid === process.pid) op.closed = true;
    }
    let cleanup = 'retained';
    if ((action === 'complete' || action === 'release') && !active(value) && (value.closed || Object.values(value.operations).every(op => op.closed))) {
      try { removeTree(scratch); cleanup = 'cleaned'; }
      catch (error) { retained.push({ entry: key, reason: error.message }); }
    }
    if (action !== 'status') {
      if (action !== 'touch' || !value.closed) value.updated = now; // retention counts from completion
      writeState(owner, value, 32768, true);
    }
    const scratchUsage = contents(scratch), evidenceUsage = contents(evidence);
    return { status: 'available', directory: folder, scratch: directory(scratch) ? cleanup : 'cleaned', scratchDirectory: scope,
      scratchUsage, evidence: { artifacts: evidenceUsage.files, registeredArtifacts: value.evidence.length, ...evidenceUsage }, state: 'existing checkpoint/session stores',
      diskBytes: scratchUsage.bytes + evidenceUsage.bytes + fs.statSync(owner).size, active: active(value), cleaned, retained };
  } finally { releaseState(ticket); }
}
