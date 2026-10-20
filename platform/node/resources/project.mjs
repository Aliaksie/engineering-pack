import os from 'node:os';
import fs from 'node:fs';
import path from 'node:path';
export function projectData(root) {
  const file = path.join(root, '.capabilities', 'project.json');
  for (const candidate of [path.dirname(file), file]) {
    const stat = fs.lstatSync(candidate, { throwIfNoEntry: false });
    if (stat?.isSymbolicLink()) throw new Error('project configuration may not be a symlink');
  }
  if (!fs.existsSync(file)) return { formatVersion: 1 };
  return JSON.parse(fs.readFileSync(file, 'utf8'));
}

// Only an explicitly selected or private, user-owned preference file is trusted.
export function preferenceData(file) {
  const stat = fs.lstatSync(file, { throwIfNoEntry: false });
  if (!stat) return {};
  const parent = fs.lstatSync(path.dirname(file));
  for (const entry of [parent, stat]) {
    if (entry.isSymbolicLink() || (process.getuid && entry.uid !== process.getuid()) || (entry.mode & 0o022)) throw new Error('preferences must be user-owned, non-symlink and not writable by others');
  }
  if (!stat.isFile() || stat.size > 65536) throw new Error('preferences must be a bounded JSON file');
  const fd = fs.openSync(file, fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW ?? 0));
  try {
    const opened = fs.fstatSync(fd);
    if (opened.dev !== stat.dev || opened.ino !== stat.ino) throw new Error('preferences changed while opening');
    const buffer = Buffer.alloc(65537);
    let bytes = 0, count;
    while (bytes < buffer.length && (count = fs.readSync(fd, buffer, bytes, buffer.length - bytes, null)) > 0) bytes += count;
    if (bytes > 65536) throw new Error('preferences must be a bounded JSON file');
    return JSON.parse(buffer.subarray(0, bytes).toString('utf8'));
  } finally { fs.closeSync(fd); }
}

export const preferenceFile = () => path.join(os.homedir(), '.config', 'craft', 'preferences.json');
