import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const OMIT = new Set(['.git', 'node_modules', 'dist']);
function safeFile(root, relative) {
  let current = root;
  for (const part of relative.split('/')) {
    current = path.join(current, part);
    if (fs.lstatSync(current).isSymbolicLink()) return null;
  }
  return current;
}

// One bounded literal scan for all terms. No model, shell interpreter or network.
export function scan(root, { terms, scopes, maxFiles, maxMatches, maxBytes, maxFileBytes, maxSnippetCharacters }, match) {
  root = fs.realpathSync(root);
  for (const entry of scopes) {
    if (!safeFile(root, entry)) throw new Error(`Symlink scope is not supported: ${entry}`);
  }
  const git = spawnSync('git', ['-c', 'core.fsmonitor=false', 'ls-files', '--cached', '--others', '--exclude-standard', '-z'], {
    cwd: root, encoding: 'utf8', shell: false, timeout: 5000, maxBuffer: 2_097_152,
    env: { ...process.env, GIT_OPTIONAL_LOCKS: '0' }
  });
  if (git.error && git.error.code !== 'ENOENT') throw new Error(`Git inventory failed: ${git.error.code}`);
  if (git.signal) throw new Error('Git inventory did not complete');
  const gitInventory = git.status === 0;
  if (!gitInventory && !scopes.length) throw new Error('Without a Git inventory, explicit paths are required');
  const truncation = new Set();
  const skipped = { symlink: 0, binary: 0, oversized: 0, unreadable: 0, excludedDirectory: 0 };
  const files = new Set();
  let entries = 0;
  const inScope = name => !scopes.length || scopes.some(entry => entry === '.' || name === entry || name.startsWith(`${entry}/`));
  const add = name => {
    if (name === '.capabilities/state' || name.startsWith('.capabilities/state/')) return;
    if (!inScope(name) || name.split('/').some(part => OMIT.has(part))) return;
    if (files.size >= maxFiles && !files.has(name)) { truncation.add('maxFiles'); return; }
    files.add(name);
  };
  if (gitInventory) {
    for (const name of git.stdout.split('\0').filter(Boolean).sort()) add(name);
  } else {
    function visit(name) {
      if (name === '.capabilities/state' || name.startsWith('.capabilities/state/')) return;
      if (++entries > maxFiles * 10) { truncation.add('traversalEntries'); return; }
      const absolute = safeFile(root, name);
      if (!absolute) { skipped.symlink++; return; }
      const stat = fs.statSync(absolute);
      if (stat.isFile()) { add(name); return; }
      if (!stat.isDirectory()) return;
      // Cap enumeration itself, including excluded entries, rather than first
      // allocating an arbitrarily large directory listing.
      const children = [];
      const directory = fs.opendirSync(absolute);
      try {
        let child;
        while ((child = directory.readSync())) {
          if (children.length >= maxFiles * 10 - entries) { truncation.add('traversalEntries'); break; }
          children.push(child.name);
        }
      } finally { directory.closeSync(); }
      for (const child of children.sort()) {
        if (OMIT.has(child)) { skipped.excludedDirectory++; continue; }
        if (entries >= maxFiles * 10) { truncation.add('traversalEntries'); break; }
        visit(name === '.' ? child : `${name}/${child}`);
      }
    }
    for (const entry of scopes) visit(entry);
  }
  const matches = [];
  let bytesRead = 0;
  let filesScanned = 0;
  for (const name of [...files].sort()) {
    if (matches.length >= maxMatches) { truncation.add('maxMatches'); break; }
    try {
      const absolute = safeFile(root, name);
      if (!absolute) { skipped.symlink++; continue; }
      const stat = fs.statSync(absolute);
      if (!stat.isFile()) continue;
      if (stat.size > maxFileBytes) { skipped.oversized++; continue; }
      if (stat.size > maxBytes - bytesRead) { truncation.add('maxBytes'); continue; }
      // Fixed read length also bounds a file that grows between stat and read.
      const descriptor = fs.openSync(absolute, 'r');
      const buffer = Buffer.alloc(stat.size);
      let count;
      try { count = fs.readSync(descriptor, buffer, 0, buffer.length, 0); } finally { fs.closeSync(descriptor); }
      bytesRead += count;
      if (buffer.subarray(0, count).includes(0)) { skipped.binary++; continue; }
      filesScanned++;
      matches.push(...match({ content: buffer.subarray(0, count).toString('utf8'), file: name, terms, maxMatches: maxMatches - matches.length }));
      if (matches.length >= maxMatches) truncation.add('maxMatches');
    } catch { skipped.unreadable++; }
  }
  return {
    inventory: gitInventory ? 'git' : 'explicit-paths', terms, paths: scopes,
    bounds: { maxFiles, maxMatches, maxBytes, maxFileBytes, maxSnippetCharacters },
    filesScanned, bytesRead, matches, skipped, truncated: truncation.size > 0, truncation: [...truncation],
    limitations: ['Case-sensitive literal matches are navigation evidence, not semantic definitions.', 'No match is not proof of absence: ignores, scopes, skips and bounds restrict coverage.', 'Symlinks, generated directories and binary or oversized files are excluded; this is not a sandbox against concurrent filesystem changes.']
  };
}
