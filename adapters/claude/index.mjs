import fs from 'node:fs';
import path from 'node:path';
import { emitHooks, mcpServers, preparePackage, report, write, writeJson } from '../shared/export.mjs';

export function emit(item, packages, output, adapterRoot) {
  const manifest = item.manifest;
  const destination = path.join(output, 'plugins', manifest.name);
  const skills = preparePackage(item, packages, destination, 'claude');
  const readme = fs.readFileSync(path.join(destination, 'README.md'), 'utf8');
  write(destination, 'README.md', `${readme}\n## Claude Code install\n\nAfter adding the generated marketplace:\n\n\`\`\`text\n/plugin install ${manifest.name}@craft-capabilities\n\`\`\`\n`);
  writeJson(destination, '.claude-plugin/plugin.json', {
    name: manifest.name, version: manifest.version, description: manifest.description, author: manifest.author,
    ...(manifest.hooks.length ? { hooks: './platform-hooks/hooks.json' } : {}),
    ...(manifest.mcp.length ? { mcpServers: './.mcp.json' } : {})
  });
  emitHooks(item, destination, 'claude', adapterRoot);
  if (manifest.mcp.length) writeJson(destination, '.mcp.json', { mcpServers: mcpServers(manifest, 'claude') });
  // Skill bodies must carry the shared context; loading an installed plugin does not imply
  // its package instructions are loaded as repository instructions.
  return report(manifest, 'claude', skills, manifest.hooks.length ? 'native-before-tool' : 'none', manifest.mcp.length ? 'native-config' : 'none');
}

export function finish(packages, output) {
  writeJson(output, '.claude-plugin/marketplace.json', {
    name: 'craft-capabilities', owner: { name: 'craft-capabilities contributors' },
    metadata: { description: 'Portable AI capability packages for engineering, delivery and agent development.' },
    plugins: packages.map(({ manifest }) => ({ name: manifest.name, source: `./plugins/${manifest.name}`, version: manifest.version, description: manifest.description }))
  });
}
