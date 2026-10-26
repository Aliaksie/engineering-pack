import { fileURLToPath } from 'node:url';
import { loadCapabilities } from './lib/capabilities.mjs';
import { validateCatalogSources } from './lib/playbook-catalog.mjs';
import { synchronizeVersion } from './version.mjs';

function inspectRepository(repository) {
  synchronizeVersion(repository, undefined, true);
  const packages = loadCapabilities(repository);
  if (packages.length === 0) throw new Error('no capability packages');
  const playbooks = validateCatalogSources(repository, packages);
  return [
    `${playbooks.entries.length} logical playbooks resolve to registered source targets.`,
    `${packages.length} capability packages are valid.`
  ];
}
try { process.stdout.write(`${inspectRepository(fileURLToPath(new URL('../', import.meta.url))).join('\n')}\n`); }
catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 1; }
