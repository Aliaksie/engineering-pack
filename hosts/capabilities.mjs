import table from './capabilities.json' with { type: 'json' };
import planning from './planning.json' with { type: 'json' };

// The same bounded data is embedded in craft; builders can read it without running
// an installed executable or importing a runtime decision implementation.
export function featureSupport(host) {
  if (!Object.hasOwn(table, host)) throw new Error('unknown host adapter');
  return structuredClone(table[host]);
}
export const executionSupport = host => featureSupport(host).execution;

// How the host's own plan approval, questions and task list relate to Craft's command path:
// what Craft observes and controls, kept apart, and which path is active.
export function planningSupport(host) {
  if (!Object.hasOwn(planning, host)) throw new Error('unknown host adapter');
  return structuredClone(planning[host]);
}
