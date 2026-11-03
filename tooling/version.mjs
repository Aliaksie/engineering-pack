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
  return ['package.json', ...packages.map(entry => `packages/${entry.name}/capability.json`)];
}
function projectionFile(root, name) {
  let current = root;
  for (const component of name.split('/')) {
    current = path.join(current, component);
    if (fs.lstatSync(current, { throwIfNoEntry: false })?.isSymbolicLink()) throw new Error('version paths may not traverse symlinks');
  }
  return current;
}

// The single-line `key = value` entry of a TOML table, with the lines to rewrite it in.
function tomlEntry(text, table, key) {
  const lines = text.split('\n');
  let inside = false;
  for (const [index, line] of lines.entries()) {
    const trimmed = line.trim();
    if (trimmed.startsWith('[')) inside = trimmed === `[${table}]`;
    else if (inside && trimmed.startsWith(key) && /^\s*=/.test(trimmed.slice(key.length))) return { lines, index, value: trimmed.slice(key.length).replace(/^\s*=\s*/, '') };
  }
  return undefined;
}

// Crates are not published, but `craft` reports the workspace version, so it follows the pack:
// every member inherits the workspace version and Cargo.lock records it for each member.
function cargoProjections(root, version) {
  const manifest = projectionFile(root, 'Cargo.toml'), lock = projectionFile(root, 'Cargo.lock');
  const entry = tomlEntry(fs.readFileSync(manifest, 'utf8'), 'workspace.package', 'version');
  const declared = entry?.value.match(/^"([^"]*)"$/)?.[1];
  if (declared === undefined) throw new Error('Cargo.toml must declare a [workspace.package] version');
  const members = tomlEntry(entry.lines.join('\n'), 'workspace', 'members')?.value.match(/^\[(.*)\]$/)?.[1];
  if (!members) throw new Error('Cargo.toml must list workspace members on one line');
  const crates = members.split(',').map(member => member.trim()).filter(Boolean).map(member => {
    const text = fs.readFileSync(projectionFile(root, `${JSON.parse(member)}/Cargo.toml`), 'utf8');
    const name = tomlEntry(text, 'package', 'name')?.value.match(/^"([^"]+)"$/)?.[1];
    if (!name || tomlEntry(text, 'package', 'version.workspace')?.value !== 'true') throw new Error(`workspace member ${member} must be named and inherit the workspace version`);
    return name;
  });
  entry.lines[entry.index] = entry.lines[entry.index].replace(/"[^"]*"/, JSON.stringify(version));
  const locked = new Map();
  const lockText = fs.readFileSync(lock, 'utf8').split('\n\n').map(block => {
    const name = block.match(/^name = "([^"]+)"$/m)?.[1];
    if (!block.startsWith('[[package]]') || !crates.includes(name) || /^source = /m.test(block)) return block;
    locked.set(name, block.match(/^version = "([^"]*)"$/m)?.[1]);
    return block.replace(/^version = "[^"]*"$/m, `version = ${JSON.stringify(version)}`);
  }).join('\n\n');
  const missing = crates.find(name => !locked.has(name));
  if (missing) throw new Error(`Cargo.lock has no entry for ${missing}`);
  return [
    { name: 'Cargo.toml', file: manifest, current: declared, text: entry.lines.join('\n') },
    { name: 'Cargo.lock', file: lock, current: [...locked.values()].find(value => value !== version) ?? version, text: lockText }
  ];
}

// `cargo: false` limits the projections to what package exports carry; the Rust workspace
// does not feed them. Version sync, version check, validation and release keep it.
export function synchronizeVersion(root, version = packVersion(root), check = false, { cargo = true } = {}) {
  parseVersion(version);
  const projections = versionFiles(root).map(name => {
    const file = projectionFile(root, name);
    const data = fs.existsSync(file) ? JSON.parse(fs.readFileSync(file, 'utf8')) : {};
    return { name, file, current: data.version, text: `${JSON.stringify({ ...data, version }, null, 2)}\n` };
  });
  // Every path is read and validated before any projection is written.
  if (cargo) projections.push(...cargoProjections(root, version));
  const stale = projections.filter(projection => projection.current !== version);
  if (check && stale.length) throw new Error(`version drift: ${stale[0].name}`);
  if (!check) for (const { file, text } of stale) fs.writeFileSync(file, text);
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
