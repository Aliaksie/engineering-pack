import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export function parseVersion(value) {
  if (typeof value !== 'string' || !/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(value)) throw new Error('expected stable SemVer');
  const parts = value.split('.').map(Number);
  if (parts.some(part => !Number.isSafeInteger(part))) throw new Error('version exceeds safe integer range');
  return parts;
}
export function incrementVersion(value, bump) {
  const parts = parseVersion(value), index = ['major', 'minor', 'patch'].indexOf(bump);
  if (index < 0) throw new Error('expected patch, minor or major');
  parts[index]++;
  for (let i = index + 1; i < 3; i++) parts[i] = 0;
  const result = parts.join('.'); parseVersion(result); return result;
}
export function packVersion(root) {
  const version = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8')).version;
  parseVersion(version); return version;
}
export function versionFiles(root) {
  const packages = fs.readdirSync(path.join(root, 'packages'), { withFileTypes: true }).sort((a,b) => a.name.localeCompare(b.name));
  if (packages.some(entry => !entry.isDirectory())) throw new Error('expected regular package directories');
  return ['package.json', ...packages.map(entry => `packages/${entry.name}/capability.json`), 'platform/node/pack-version.json'];
}
export function synchronizeVersion(root, version = packVersion(root), check = false) {
  parseVersion(version);
  const paths = versionFiles(root);
  const updates = [];
  for (const name of paths) {
    const file = path.join(root, name);
    let current = root;
    for (const component of name.split('/')) {
      current = path.join(current,component);
      if (fs.lstatSync(current, { throwIfNoEntry: false })?.isSymbolicLink()) throw new Error('version paths may not traverse symlinks');
    }
    const data = fs.existsSync(file) ? JSON.parse(fs.readFileSync(file, 'utf8')) : {};
    if (check && data.version !== version) throw new Error(`version drift: ${name}`);
    if (!check && data.version !== version) updates.push([file, `${JSON.stringify({ ...data, version }, null, 2)}\n`]);
  }
  for (const [file,text] of updates) fs.writeFileSync(file,text);
  return version;
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const root = fileURLToPath(new URL('../', import.meta.url));
  const [value, ...extra] = process.argv.slice(2);
  if (extra.length) throw new Error('usage: node tooling/version.mjs [--check|patch|minor|major|X.Y.Z]');
  const current = packVersion(root);
  const version = !value || value === '--check' ? current : ['patch','minor','major'].includes(value) ? incrementVersion(current,value) : value;
  process.stdout.write(`${synchronizeVersion(root,version,value === '--check')}\n`);
}
