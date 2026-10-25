import fs from 'node:fs';

// Hook mapping data shared with the native runtime, which embeds the same file.
// Observing an event is not the same as injecting context: support is explicit per event.
export const HOSTS = JSON.parse(fs.readFileSync(new URL('./hooks.json', import.meta.url), 'utf8'));
export function hookMapping(platform, definition) {
  const mapping = HOSTS[platform]?.events[definition.event];
  return mapping ? { ...mapping, event: definition.event, tool: definition.tool, handler: definition.handler, id: definition.name } : null;
}
