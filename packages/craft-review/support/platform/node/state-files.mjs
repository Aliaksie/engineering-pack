import fs from 'node:fs';
import path from 'node:path';
import { randomUUID } from 'node:crypto';

export function stateFile(root, name, create = false) {
  if (!/^[a-zA-Z0-9.-]+$/.test(name)) throw new Error('invalid state filename');
  let directory = path.resolve(root);
  for (const part of ['.capabilities', 'state']) {
    directory = path.join(directory, part);
    if (fs.lstatSync(directory, { throwIfNoEntry: false })?.isSymbolicLink()) throw new Error('state path may not traverse symlinks');
    if (create) fs.mkdirSync(directory, { recursive: true });
  }
  return path.join(directory, name);
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
