import fs from 'node:fs';
import path from 'node:path';
import { randomUUID, createHash } from 'node:crypto';

function gitDirectory(root) {
  let directory = root;
  for (let depth = 0; depth < 64; depth++) {
    const marker = path.join(directory, '.git');
    const stat = fs.lstatSync(marker, { throwIfNoEntry: false });
    if (stat) {
      if (stat.isSymbolicLink()) throw new Error('Git metadata may not be a symlink');
      if (stat.isDirectory()) return marker;
      if (!stat.isFile() || stat.size > 4096) throw new Error('invalid Git directory marker');
      const match = /^gitdir: ([^\r\n]+)\r?\n?$/.exec(fs.readFileSync(marker, 'utf8'));
      if (!match) throw new Error('invalid Git directory marker');
      const target = path.resolve(directory, match[1]);
      if (!fs.lstatSync(target).isDirectory() || fs.lstatSync(target).isSymbolicLink()) throw new Error('invalid Git directory');
      return target;
    }
    const parent = path.dirname(directory);
    if (parent === directory) return null;
    directory = parent;
  }
  throw new Error('state root exceeds directory search limit');
}
const hash = value => createHash('sha256').update(value).digest('hex').slice(0, 24);
function directory(file, create) {
  const stat = fs.lstatSync(file, { throwIfNoEntry: false });
  if (stat && (!stat.isDirectory() || stat.isSymbolicLink())) throw new Error('state path must be a directory without symlinks');
  if (create && !stat) {
    try { fs.mkdirSync(file, { mode: 0o700 }); }
    catch (error) { if (error.code !== 'EEXIST') throw error; }
    directory(file, false);
  }
}
export function stateLocation(root, session) {
  if (session !== undefined && (typeof session !== 'string' || !session || session.length > 512)) throw new Error('invalid state session');
  const canonical = fs.realpathSync.native(root);
  const worktreeId = hash(canonical);
  const sessionId = session === undefined ? 'shared' : hash(String(session));
  const base = path.join(canonical, '.craft', 'state', worktreeId);
  let folder = canonical;
  for (const part of ['.craft', 'state', worktreeId, sessionId]) { folder = path.join(folder, part); directory(folder, false); }
  return { directory: path.join(base, sessionId), scope: 'workspace-local', formatVersion: 2, worktreeId, sessionId,
    legacyDirectory: path.join(canonical, '.craft', 'state') };
}
export function stateFile(root, name, create = false, session) {
  if (!/^[a-zA-Z0-9.-]+$/.test(name) || name === '.' || name === '..') throw new Error('invalid state filename');
  const canonical = fs.realpathSync.native(root);
  const location = stateLocation(canonical, session);
  let folder = canonical;
  for (const part of path.relative(canonical, location.directory).split(path.sep)) {
    folder = path.join(folder, part);
    directory(folder, create);
  }
  const file = path.join(folder, name);
  migrateLegacy(canonical, file, name);
  return file;
}

// Legacy stores are read-only. Copy on first access and retain a source digest so an
// older plugin resuming writes cannot silently fork history. Consumers must stop old
// writers before upgrading; a legacy lock or changed source requires reconciliation.
function migrateLegacy(root, target, name) {
  const git = gitDirectory(root);
  const sources = [
    ...(git ? [path.join(git, 'craft-state', hash(root))] : []),
    path.join(root, '.craft', 'state'), path.join(root, '.capabilities', 'state')
  ];
  for (const source of sources) {
    // Validate every ancestor, including historical namespaces, before reading.
    let ancestor = source;
    while (ancestor !== path.dirname(ancestor)) {
      const stat = fs.lstatSync(ancestor, { throwIfNoEntry: false });
      if (stat && (!stat.isDirectory() || stat.isSymbolicLink())) throw new Error('legacy state path must be a directory without symlinks');
      ancestor = path.dirname(ancestor);
    }
    const old = path.join(source, name);
    const companions = name === 'workspace-guard.json' ? [name, 'sessions'] : name === 'telemetry.jsonl' ? [name, `${name}.1`] : name.startsWith('routing-') ? [name, `${name}.generation`] : [name];
    const present = companions.filter(item => fs.lstatSync(path.join(source, item), { throwIfNoEntry: false }));
    if (!present.length) continue;
    const entries = [];
    let bytes = 0;
    const inspect = (file, relative, depth = 0) => {
      const stat = fs.lstatSync(file);
      if (depth > 16 || entries.length >= 4096 || stat.isSymbolicLink() || stat.nlink > 1 && stat.isFile()) throw new Error('unsafe or oversized legacy state');
      if (file.endsWith('.lock')) throw new Error('legacy state writer is busy; stop older plugins before migration');
      if (stat.isDirectory()) {
        entries.push({ relative, directory: true });
        for (const child of fs.readdirSync(file).sort()) inspect(path.join(file, child), path.join(relative, child), depth + 1);
      } else {
        if (!stat.isFile() || stat.size > 4194304 || (bytes += stat.size) > 16777216) throw new Error('unsafe or oversized legacy state');
        const data = fs.readFileSync(file);
        const after = fs.lstatSync(file);
        if (data.length !== stat.size || after.ino !== stat.ino || after.size !== stat.size || after.mtimeMs !== stat.mtimeMs) throw new Error('legacy state changed during migration');
        if (depth === 0 && file.endsWith('.json')) JSON.parse(data.toString('utf8'));
        entries.push({ relative, data, digest: hash(data) });
      }
    };
    // Flat legacy stores use per-record locks; any live/unresolved writer blocks migration.
    if (fs.readdirSync(source).some(item => item.endsWith('.lock'))) throw new Error('legacy state writer is busy; stop older plugins before migration');
    for (const item of present) inspect(path.join(source, item), item);
    const fingerprint = hash(JSON.stringify(entries.map(({ relative, directory, digest }) => ({ relative, directory, digest }))));
    const destination = path.dirname(target);
    const receipt = path.join(destination, `migration-${hash(old)}.json`);
    const previous = readState(receipt, 4096);
    if (previous) {
      if (previous.formatVersion !== 1 || previous.source !== fingerprint) throw new Error('legacy state changed after migration; stop older plugins and reconcile stores');
      continue;
    }
    // Create missing destination parents only when there is actual legacy data.
    let folder = root;
    for (const part of path.relative(root, destination).split(path.sep)) { folder = path.join(folder, part); directory(folder, true); }
    const ticket = lockState(receipt);
    if (!ticket) throw new Error('state migration is busy');
    try {
      const current = readState(receipt, 4096);
      if (current) {
        if (current.source !== fingerprint) throw new Error('legacy migration source conflict');
        continue;
      }
      // Identical files from an interrupted copy are reusable; differing destinations
      // are never overwritten. Publish the receipt only after the complete copy.
      for (const item of entries) {
        const file = path.join(destination, item.relative);
        if (item.directory) directory(file, true);
        else {
          const stat = fs.lstatSync(file, { throwIfNoEntry: false });
          if (stat) {
            if (!stat.isFile() || stat.isSymbolicLink() || stat.size !== item.data.length || hash(fs.readFileSync(file)) !== item.digest) throw new Error('legacy state destination conflict');
          } else {
            const temporary = `${file}.${randomUUID()}.tmp`;
            try {
              fs.writeFileSync(temporary, item.data, { flag: 'wx', mode: 0o600 });
              // Exclusive atomic publication: a crash cannot leave a partial destination.
              fs.linkSync(temporary, file);
            } finally { fs.rmSync(temporary, { force: true }); }
          }
        }
      }
      writeState(receipt, { formatVersion: 1, source: fingerprint }, 4096);
    } finally { releaseState(ticket); }
  }
}
export function readState(file, maxBytes) {
  const stat = fs.lstatSync(file, { throwIfNoEntry: false });
  if (!stat) return null;
  if (stat.isSymbolicLink() || !stat.isFile() || stat.size > maxBytes) throw new Error('invalid bounded state file');
  return JSON.parse(fs.readFileSync(file, 'utf8'));
}
// Windows denies creating a lock its previous holder is still deleting (busy); without a listed lock the create is retried, then the denial is real.
const denied = error => process.platform === 'win32' && ['EPERM', 'EACCES'].includes(error.code);
export function lockState(file, attempts = 3) {
  try { return { file, fd: fs.openSync(`${file}.lock`, 'wx', 0o600) }; }
  catch (error) { if (error.code === 'EEXIST' || denied(error) && fs.readdirSync(path.dirname(file)).includes(`${path.basename(file)}.lock`)) return null; if (!denied(error) || attempts <= 1) throw error; return lockState(file, attempts - 1); }
}
export function releaseState(ticket) { fs.closeSync(ticket.fd); fs.rmSync(`${ticket.file}.lock`, { force: true }); }
export function writeState(file, state, maxBytes, pretty = false) {
  const text = `${JSON.stringify(state, null, pretty ? 2 : undefined)}\n`;
  if (Buffer.byteLength(text) > maxBytes) throw new Error('state exceeds size limit');
  readState(file, maxBytes); // Recheck including dangling symlinks before atomic replacement.
  const temporary = `${file}.${randomUUID()}.tmp`;
  try {
    fs.writeFileSync(temporary, text, { flag: 'wx', mode: 0o600 });
    fs.renameSync(temporary, file);
  } finally { fs.rmSync(temporary, { force: true }); }
}
