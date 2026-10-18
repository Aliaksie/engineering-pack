import { lstatSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { validateReferences } from './lib/cross-references.mjs';
import { validatePlugin } from './lib/plugin-manifest.mjs';

const record = value => value !== null && typeof value === 'object' && !Array.isArray(value);
const catalogRules = [
  [value => typeof value?.name === 'string' && /^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(value.name), 'marketplace: name must be non-empty kebab-case'],
  [value => typeof value?.owner?.name === 'string' && value.owner.name.trim(), 'marketplace: owner.name is required'],
  [value => Array.isArray(value?.plugins) && value.plugins.length > 0, 'marketplace: plugins must be a non-empty array']
];

export function validateCatalog(root, catalog) {
  const diagnostics = catalogRules.filter(([accept]) => !accept(catalog)).map(([, message]) => message);
  if (!Array.isArray(catalog?.plugins)) return diagnostics;

  const entries = catalog.plugins.filter(record);
  const names = entries.map(entry => entry.name);
  const inspections = catalog.plugins.flatMap(entry => {
    if (!record(entry)) return ['marketplace: plugin entry must be an object'];
    const findings = [];
    validatePlugin(root, entry, findings);
    return findings;
  });
  const duplicates = [...new Set(names.filter((name, index) => typeof name === 'string' && names.indexOf(name) !== index))]
    .map(name => `marketplace: duplicate plugin name ${name}`);
  const references = validateReferences(root, { plugins: entries }, []);
  return [...diagnostics, ...duplicates, ...inspections, ...references];
}

export function validateMarketplace(root) {
  const components = [path.join(root, '.claude-plugin'), path.join(root, '.claude-plugin', 'marketplace.json')];
  let catalog;
  try {
    const metadata = components.map(filename => lstatSync(filename));
    if (metadata.some(stat => stat.isSymbolicLink())) return ['marketplace: symbolic links are not accepted for catalog files'];
    if (!metadata[0].isDirectory() || !metadata[1].isFile()) return ['marketplace: expected a catalog file in .claude-plugin'];
    catalog = readFileSync(components[1], 'utf8');
  } catch (error) {
    return [error.code === 'ENOENT' ? 'marketplace: missing .claude-plugin/marketplace.json' : `marketplace: cannot read catalog (${error.message})`];
  }
  try { catalog = JSON.parse(catalog); }
  catch (error) { return [`marketplace: invalid JSON (${error.message})`]; }
  return validateCatalog(root, catalog);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  const diagnostics = validateMarketplace(path.resolve(process.argv[2] ?? '.'));
  process.exitCode = diagnostics.length ? 1 : 0;
  (diagnostics.length ? process.stderr : process.stdout).write(diagnostics.length
    ? `${diagnostics.map(message => `ERROR: ${message}`).join('\n')}\n`
    : 'Marketplace structure is valid.\n');
}
