import path from 'node:path';
import { emitHooks, mcpServers, preparePackage, report, writeJson } from '../export.mjs';

export function emit(item, packages, output, adapterRoot) {
  const manifest = item.manifest;
  const destination = path.join(output, 'packages', manifest.name);
  const skills = preparePackage(item, packages, destination, 'codex');
  // Codex 0.163 skips hook loading for Agent Plugins 1.0 root manifests,
  // even when a native fallback exists (openai/codex#47925). Emit only the
  // native manifest here; canonical portable content remains in packages/.
  writeJson(destination, '.codex-plugin/plugin.json', {
    name: manifest.name, version: manifest.version, description: manifest.description, author: manifest.author,
    ...(manifest.hooks.length ? { hooks: './platform-hooks/hooks.json' } : {}),
    ...(manifest.mcp.length ? { mcpServers: './.mcp.json' } : {})
  });
  emitHooks(item, destination, 'codex', adapterRoot);
  if (manifest.mcp.length) writeJson(destination, '.mcp.json', {
    mcpServers: mcpServers(manifest, 'codex')
  });
  return report(manifest, 'codex', skills, manifest.hooks.length ? 'native-before-tool-requires-trust' : 'none', manifest.mcp.length ? 'native-config' : 'none');
}
export function finish(packages, output) {
  writeJson(output, '.agents/plugins/marketplace.json', {
    name: 'engineering-pack',
    interface: { displayName: 'Craft Capabilities' },
    plugins: packages.map(({ manifest }) => ({
      name: manifest.name,
      source: { source: 'local', path: `./packages/${manifest.name}` },
      policy: { installation: 'AVAILABLE', authentication: 'ON_INSTALL' },
      category: 'Productivity'
    }))
  });
}
