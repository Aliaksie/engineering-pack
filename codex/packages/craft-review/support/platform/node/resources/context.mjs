import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { privatePath } from '../private-path.mjs';

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
  const supplied = Array.isArray(bundleRoot) ? bundleRoot : [bundleRoot];
  if (supplied.length < 1 || supplied.length > 16 || supplied.some(root => typeof root !== 'string' || !root.trim())) throw new Error('context reporting requires 1 to 16 explicit bundle roots');
  const roots = [...new Set(supplied.map(root => fs.realpathSync(root)))];
  if (roots.some(root => privatePath(root.split(path.sep).join('/')))) throw new Error('private workspace cannot supply guidance');
  const files = new Map();
  const missing = [];
  const packages = new Map();
  function packageRoot(pkg) {
    if (packages.has(pkg)) return packages.get(pkg);
    const locations = new Map();
    for (const root of roots) for (const prefix of ['', `packages/${pkg}/`, `plugins/${pkg}/`]) {
      for (const manifest of ['capability.json', 'source-capability.json']) {
        const file = localFile(root, `${prefix}${manifest}`);
        if (file && JSON.parse(fs.readFileSync(file, 'utf8')).name === pkg) {
          locations.set(path.join(root, prefix), { root, prefix, generated: manifest === 'source-capability.json' });
          break;
        }
      }
    }
    const result = locations.size > 1 ? { ambiguous: true } : locations.values().next().value ?? null;
    packages.set(pkg, result);
    return result;
  }
  function record(root, relative, target, selectedBy) {
    const file = localFile(root, relative);
    if (!file) { missing.push({ target, reason: 'file not present in this bundle' }); return; }
    if (files.has(file)) {
      const entry = files.get(file);
      entry.selections++;
      if (!entry.selectedBy.includes(selectedBy)) entry.selectedBy.push(selectedBy);
      return;
    }
    const content = fs.readFileSync(file);
    if (content.length > MAX_FILE_BYTES) throw new Error(`context file exceeds read limit: ${relative}`);
    files.set(file, { target, path: relative, ...(roots.length > 1 ? { bundleRoot: root } : {}), bytes: content.length,
      sha256: createHash('sha256').update(content).digest('hex'),
      estimatedTokens: Math.ceil(content.length / 4), selections: 1, selectedBy: [selectedBy] });
  }
  for (const { target, selectedBy } of requests) {
    const match = targetPattern.exec(target);
    if (!match) throw new Error(`invalid context target: ${target}`);
    const [, pkg, name, reference] = match;
    const location = packageRoot(pkg);
    if (!location) { missing.push({ target, reason: 'package not present in supplied bundle roots' }); continue; }
    if (location.ambiguous) { missing.push({ target, reason: 'package present in multiple supplied roots; select one installation' }); continue; }
    const { root, prefix, generated } = location;
    const plain = `skills/${name}${reference ?? '/SKILL.md'}`;
    const namespaced = `skills/${pkg}-${name}${reference ?? '/SKILL.md'}`;
    let relative = plain;
    if (generated) relative = localFile(root, `${prefix}${namespaced}`) ? namespaced : plain;
    else if (!reference && name.startsWith('workflow-')) relative = `workflows/${name.slice('workflow-'.length)}.md`;
    record(root, `${prefix}${relative}`, target, selectedBy);
    if (!reference) record(root, `${prefix}AGENTS.md`, `${pkg}:instructions`, selectedBy);
  }
  return { entries: [...files.values()], missing };
}
