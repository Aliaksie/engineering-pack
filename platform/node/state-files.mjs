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
export function stateFile(root, name, create = false) {
  if (!/^[a-zA-Z0-9.-]+$/.test(name) || name === '.' || name === '..') throw new Error('invalid state filename');
  const canonical = fs.realpathSync(root);
  const git = gitDirectory(canonical);
  let directory = git ?? canonical;
  const namespace = createHash('sha256').update(canonical).digest('hex').slice(0, 24);
  // Worktree-specific metadata is excluded by Git without editing consumer ignores.
  const parts = git ? ['craft-state', namespace] : ['.capabilities', 'state'];
  for (const part of parts) {
    directory = path.join(directory, part);
    const stat = fs.lstatSync(directory, { throwIfNoEntry: false });
    if (stat && (!stat.isDirectory() || stat.isSymbolicLink())) throw new Error('state path must be a directory without symlinks');
    if (create && !stat) {
      try { fs.mkdirSync(directory, { mode: 0o700 }); }
      catch (error) { if (error.code !== 'EEXIST') throw error; }
      const created = fs.lstatSync(directory);
      if (!created.isDirectory() || created.isSymbolicLink()) throw new Error('state path must be a directory without symlinks');
    }
  }
  return path.join(directory, name);
}
export function stateLocation(root) {
  const canonical = fs.realpathSync(root);
  return { directory: path.dirname(stateFile(canonical, 'location.json')), scope: gitDirectory(canonical) ? 'git-private' : 'workspace-local', legacyDirectory: path.join(canonical, '.capabilities', 'state') };
}
export function readState(file, maxBytes) {
  const stat = fs.lstatSync(file, { throwIfNoEntry: false });
  if (!stat) return null;
  if (stat.isSymbolicLink() || !stat.isFile() || stat.size > maxBytes) throw new Error('invalid bounded state file');
  return JSON.parse(fs.readFileSync(file, 'utf8'));
}
export function lockState(file) {
  try { return { file, fd: fs.openSync(`${file}.lock`, 'wx', 0o600) }; }
  catch (error) { if (error.code === 'EEXIST') return null; throw error; }
}
export function releaseState(ticket) {
  fs.closeSync(ticket.fd); fs.rmSync(`${ticket.file}.lock`, { force: true });
}
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
