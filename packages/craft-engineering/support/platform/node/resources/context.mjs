import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';

const slug = '[a-z0-9]+(?:-[a-z0-9]+)*';
const targetPattern = new RegExp(`^(${slug}):(${slug})(/references/(?:${slug}/)*${slug}\\.md)?$`);
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

export function contextFiles(bundleRoot, requests) {
  const root = fs.realpathSync(bundleRoot);
  const files = new Map();
  const missing = [];
  const packages = new Map();
  function packageRoot(pkg) {
    if (packages.has(pkg)) return packages.get(pkg);
    for (const prefix of ['', `packages/${pkg}/`, `plugins/${pkg}/`]) {
      for (const manifest of ['capability.json', 'source-capability.json']) {
        const file = localFile(root, `${prefix}${manifest}`);
        if (file && JSON.parse(fs.readFileSync(file, 'utf8')).name === pkg) {
          const result = { prefix, generated: manifest === 'source-capability.json' };
          packages.set(pkg, result);
          return result;
        }
      }
    }
    packages.set(pkg, null);
    return null;
  }
  function record(relative, target, selectedBy) {
    const file = localFile(root, relative);
    if (!file) { missing.push({ target, reason: 'file not present in this bundle' }); return; }
    if (files.has(relative)) {
      const entry = files.get(relative);
      entry.selections++;
      if (!entry.selectedBy.includes(selectedBy)) entry.selectedBy.push(selectedBy);
      return;
    }
    const content = fs.readFileSync(file);
    if (content.length > MAX_FILE_BYTES) throw new Error(`context file exceeds read limit: ${relative}`);
    files.set(relative, { target, path: relative, bytes: content.length,
      sha256: createHash('sha256').update(content).digest('hex'),
      estimatedTokens: Math.ceil(content.length / 4), selections: 1, selectedBy: [selectedBy] });
  }
  for (const { target, selectedBy } of requests) {
    const match = targetPattern.exec(target);
    if (!match) throw new Error(`invalid context target: ${target}`);
    const [, pkg, name, reference] = match;
    const location = packageRoot(pkg);
    if (!location) { missing.push({ target, reason: 'package not present in this bundle' }); continue; }
    const { prefix, generated } = location;
    const plain = `skills/${name}${reference ?? '/SKILL.md'}`;
    const namespaced = `skills/${pkg}-${name}${reference ?? '/SKILL.md'}`;
    let relative = plain;
    if (generated) relative = localFile(root, `${prefix}${namespaced}`) ? namespaced : plain;
    else if (!reference && name.startsWith('workflow-')) relative = `workflows/${name.slice('workflow-'.length)}.md`;
    record(`${prefix}${relative}`, target, selectedBy);
    if (!reference) record(`${prefix}AGENTS.md`, `${pkg}:instructions`, selectedBy);
  }
  return { entries: [...files.values()], missing };
}
