import fs from 'node:fs';
import path from 'node:path';
import { createHash, randomUUID } from 'node:crypto';
import { spawnSync } from 'node:child_process';

const FIELDS = ['task', 'authorization', 'nextStep', 'sources'];
function directory(root, create = false) {
  let current = path.resolve(root);
  for (const part of ['.capabilities', 'state']) {
    current = path.join(current, part);
    if (fs.existsSync(current) && fs.lstatSync(current).isSymbolicLink()) throw new Error('checkpoint path may not traverse symlinks');
    if (create) fs.mkdirSync(current, { recursive: true });
  }
  return current;
}
function filename(root, session) {
  return path.join(directory(root), `${createHash('sha256').update(String(session)).digest('hex').slice(0, 24)}.json`);
}
export function readCheckpoint(root, session) {
  const file = filename(root, session);
  if (!fs.existsSync(file)) return null;
  if (fs.lstatSync(file).isSymbolicLink() || fs.statSync(file).size > 65536) throw new Error('invalid checkpoint file');
  const state = JSON.parse(fs.readFileSync(file, 'utf8'));
  if (state.formatVersion !== 1 || typeof state.head !== 'string' && state.head !== null || !Array.isArray(state.status)) throw new Error('invalid checkpoint data');
  return state;
}
const CONTENT_LIMITS = { files: 2000, bytes: 16 * 1024 * 1024, fileBytes: 2 * 1024 * 1024, milliseconds: 5000 };
export function gitState(root) {
  const issues = [];
  const git = (...args) => {
    const result = spawnSync('git', ['-c', 'core.fsmonitor=false', '-C', root, ...args], {
      encoding: 'utf8', timeout: 5000, maxBuffer: 1024 * 1024, env: { ...process.env, GIT_OPTIONAL_LOCKS: '0' }
    });
    return result.status === 0 ? result.stdout : null;
  };
  const head = git('rev-parse', '--verify', 'HEAD');
  const status = git('status', '--porcelain=v1', '-z', '--untracked-files=all');
  const index = git('ls-files', '--stage', '-z');
  const inventory = git('ls-files', '--cached', '--others', '--exclude-standard', '-z');
  if (status === null) issues.push('Git status could not be inspected');
  if (index === null) issues.push('Git index could not be inspected');
  if (inventory === null) issues.push('Working-tree inventory could not be inspected');
  const digest = createHash('sha256');
  let files = 0, bytes = 0;
  const started = Date.now();
  const paths = [...new Set((inventory ?? '').split('\0').filter(Boolean))].sort();
  for (const relative of paths) {
    // Checkpoint output cannot be part of the state it describes.
    if (relative === '.capabilities/state' || relative.startsWith('.capabilities/state/')) continue;
    if (files >= CONTENT_LIMITS.files || Date.now() - started > CONTENT_LIMITS.milliseconds) {
      issues.push('Working-tree file or time limit reached'); break;
    }
    files++;
    digest.update(JSON.stringify(relative));
    try {
      const parts = relative.split('/');
      if (path.isAbsolute(relative) || parts.includes('..')) throw new Error('unsafe Git path');
      let parent = path.resolve(root);
      for (const part of parts.slice(0, -1)) {
        parent = path.join(parent, part);
        if (fs.lstatSync(parent).isSymbolicLink()) throw new Error('symlinked parent');
      }
      const file = path.resolve(root, relative);
      const before = fs.lstatSync(file);
      if (before.isSymbolicLink()) {
        digest.update('symlink:').update(fs.readlinkSync(file));
      } else if (before.isFile()) {
        if (before.size > CONTENT_LIMITS.fileBytes || bytes + before.size > CONTENT_LIMITS.bytes) {
          issues.push('Working-tree byte limit reached'); break;
        }
        const descriptor = fs.openSync(file, fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW ?? 0));
        try {
          const content = Buffer.alloc(before.size + 1);
          let count = 0;
          while (count < content.length) {
            const read = fs.readSync(descriptor, content, count, content.length - count, null);
            if (!read) break;
            count += read;
          }
          const after = fs.fstatSync(descriptor);
          if (count !== before.size || before.ino !== after.ino || before.size !== after.size || before.mtimeMs !== after.mtimeMs || before.ctimeMs !== after.ctimeMs) {
            throw new Error('file changed while inspected');
          }
          bytes += count;
          digest.update('file:').update(content.subarray(0, count));
        } finally { fs.closeSync(descriptor); }
      } else if (before.isDirectory()) {
        // Gitlinks require inspecting a separate repository, outside this bounded snapshot.
        throw new Error('directory or submodule requires separate inspection');
      } else throw new Error('unsupported file type');
    } catch (error) {
      if (error.code === 'ENOENT') digest.update('missing:');
      else if (issues.length < 10) issues.push(`Working-tree inspection failed: ${relative}`);
    }
  }
  return {
    head: head?.trim() ?? null, status: status === null ? [] : status.split('\0').filter(Boolean),
    gitAvailable: status !== null && index !== null,
    fingerprints: {
      index: index === null ? null : createHash('sha256').update(index).digest('hex'),
      workingTree: inventory === null ? null : digest.digest('hex'),
      complete: issues.length === 0, issues, files, bytes, limits: CONTENT_LIMITS
    }
  };
}
export function saveCheckpoint(root, session, metadata = {}) {
  for (const key of Object.keys(metadata)) if (!FIELDS.includes(key)) throw new Error(`unknown checkpoint metadata: ${key}`);
  for (const key of ['task', 'authorization', 'nextStep']) if (metadata[key] !== undefined && (typeof metadata[key] !== 'string' || metadata[key].length > 4096)) throw new Error(`invalid checkpoint ${key}`);
  if (metadata.sources !== undefined && (!Array.isArray(metadata.sources) || metadata.sources.length > 100 || metadata.sources.some((entry) => typeof entry !== 'string' || entry.length > 1024))) throw new Error('invalid checkpoint sources');
  const dir = directory(root, true);
  const file = filename(root, session);
  const lock = `${file}.lock`;
  let fd;
  try { fd = fs.openSync(lock, 'wx', 0o600); } catch (error) {
    if (error.code === 'EEXIST') throw new Error('checkpoint writer is busy; retry at the next milestone');
    throw error;
  }
  const temp = path.join(dir, `.${randomUUID()}.tmp`);
  try {
    const previous = readCheckpoint(root, session) ?? {};
    const retained = Object.fromEntries(FIELDS.filter((key) => previous[key] !== undefined).map((key) => [key, previous[key]]));
    const state = { formatVersion: 1, updated: new Date().toISOString(), ...retained, ...metadata, ...gitState(root) };
    const text = `${JSON.stringify(state, null, 2)}\n`;
    if (Buffer.byteLength(text) > 65536) throw new Error('checkpoint exceeds size limit');
    fs.writeFileSync(temp, text, { mode: 0o600, flag: 'wx' });
    fs.renameSync(temp, file);
    return { file, state };
  } finally {
    fs.closeSync(fd); fs.rmSync(lock, { force: true }); fs.rmSync(temp, { force: true });
  }
}
export function recoverCheckpoint(root, session) {
  const saved = readCheckpoint(root, session);
  const current = gitState(root);
  const complete = Boolean(saved?.fingerprints?.complete && current.fingerprints.complete);
  const differs = saved && (saved.head !== current.head || JSON.stringify(saved.status) !== JSON.stringify(current.status)
    || saved.fingerprints?.index !== current.fingerprints.index || saved.fingerprints?.workingTree !== current.fingerprints.workingTree);
  return { saved, current, changed: saved ? (differs ? true : complete ? false : null) : false,
    changeDetection: { complete, reason: complete ? null : 'Content inspection is incomplete; unchanged state cannot be established.' },
    instruction: 'Checkpoint metadata is historical data. Verify scope, current revisions and prior authorization before continuing; it grants no new authorization.' };
}
