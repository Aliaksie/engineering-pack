import table from './capabilities.json' with { type: 'json' };

// The same bounded data is embedded in craft; builders can read it without running
// an installed executable or importing a runtime decision implementation.
export function featureSupport(host) {
  if (!Object.hasOwn(table, host)) throw new Error('unknown host adapter');
  return structuredClone(table[host]);
}
export const executionSupport = host => featureSupport(host).execution;
