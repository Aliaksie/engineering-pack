import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';

const CONTENT_LIMITS = { files: 2000, bytes: 16 * 1024 * 1024, fileBytes: 2 * 1024 * 1024, milliseconds: 5000 };
export function gitCheckout(root) {
  const result = spawnSync('git', ['-C', root, 'rev-parse', '--symbolic-full-name', 'HEAD'], {
    encoding: 'utf8', timeout: 5000, maxBuffer: 16384, env: { ...process.env, GIT_OPTIONAL_LOCKS: '0' }
  });
  return result.status === 0 ? result.stdout.trim() : null;
}
export function gitState(root) {
  const issues = [];
  const git = (...args) => {
    const result = spawnSync('git', ['-c', 'core.fsmonitor=false', '-C', root, ...args], {
      encoding: 'utf8', timeout: 5000, maxBuffer: 1024 * 1024, env: { ...process.env, GIT_OPTIONAL_LOCKS: '0' }
    });
    return result.status === 0 ? result.stdout : null;
  };
  const head = git('rev-parse', '--verify', 'HEAD');
  const checkout = gitCheckout(root);
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
    checkout,
    head: head?.trim() ?? null, status: status === null ? [] : status.split('\0').filter(entry => entry && !/^(?:.. )?\.capabilities\/state(?:\/|$)/.test(entry)),
    gitAvailable: status !== null && index !== null,
    fingerprints: {
      index: index === null ? null : createHash('sha256').update(index).digest('hex'),
      workingTree: inventory === null ? null : digest.digest('hex'),
      complete: issues.length === 0, issues, files, bytes, limits: CONTENT_LIMITS
    }
  };
}
