import path from 'node:path';
import { emitHooks, mcpServers, preparePackage, report, writeJson } from '../shared/export.mjs';

export function emit(item, packages, output, adapterRoot) {
  const manifest = item.manifest;
  const destination = path.join(output, 'packages', manifest.name);
  const skills = preparePackage(item, packages, destination, 'codex');
  writeJson(destination, 'plugin.json', {
    $schema: 'https://agent-plugins.org/schemas/1.0.0/plugin.schema.json',
    name: manifest.name, version: manifest.version, description: manifest.description, author: manifest.author,
    ...(manifest.hooks.length ? { extensions: { 'com.openai': { hooks: './platform-hooks/hooks.json' } } } : {})
  });
  emitHooks(item, destination, 'codex', adapterRoot);
  if (manifest.mcp.length) writeJson(destination, 'mcp.json', {
    $schema: 'https://agent-plugins.org/schemas/1.0.0/mcp.schema.json', mcpServers: mcpServers(manifest, 'codex')
  });
  return report(manifest, 'codex', skills, manifest.hooks.length ? 'native-before-tool-requires-trust' : 'none', manifest.mcp.length ? 'native-config' : 'none');
}
export function finish() {}
