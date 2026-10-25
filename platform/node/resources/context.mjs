import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { privatePath } from '../private-path.mjs';

const MAX_FILE_BYTES = 1024 * 1024;

// Explicit roots only; never search a user's plugin cache or follow linked resources.
function localFile(root, relative) {
  let current = root;
  for (const part of relative.split('/')) {
    current = path.join(current, part);
    const stat = fs.lstatSync(current, { throwIfNoEntry: false });
    if (!stat) return null;
    if (stat.isSymbolicLink()) throw new Error(`symlink in context path: ${relative}`);
  }
  const stat = fs.statSync(current);
  if (!stat.isFile() || stat.size > MAX_FILE_BYTES) throw new Error(`context file is not a bounded regular file: ${relative}`);
  return current;
}

// Observe explicit local citations only; source text is never executed or interpreted here.
export function groundingFiles(root, files) {
  root = fs.realpathSync(root);
  return files.map(reference => {
    if (typeof reference !== 'string' || reference.startsWith('/') || /^[A-Za-z]:/.test(reference) || reference.includes('\\') || reference.split('/').some(part => !part || part === '..') || privatePath(reference)) throw new Error('invalid grounding source path');
    try {
      const file = localFile(root, reference);
      if (!file) return { reference, identity: null, status: 'missing' };
      const content = fs.readFileSync(file);
      if (content.length > MAX_FILE_BYTES) throw new Error('grounding read limit');
      return { reference, identity: createHash('sha256').update(content).digest('hex'), status: 'observed' };
    } catch (error) { return { reference, identity: null, status: error.code === 'ENOTDIR' ? 'missing' : 'unavailable' }; }
  });
}
