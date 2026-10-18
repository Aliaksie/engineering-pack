import { OPERATIONS, WRITE_OPERATIONS, validateProject } from '../contracts/project.mjs';

function substitute(value, params) {
  if (Array.isArray(value)) return value.map((entry) => substitute(entry, params));
  if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).map(([key, entry]) => [key, substitute(entry, params)]));
  if (typeof value !== 'string') return value;
  const whole = /^\{([a-zA-Z][\w]*)\}$/.exec(value);
  if (whole) {
    if (!Object.hasOwn(params, whole[1])) throw new Error(`missing parameter ${whole[1]}`);
    return params[whole[1]];
  }
  return value.replace(/\{([a-zA-Z][\w]*)\}/g, (_, name) => {
    if (!Object.hasOwn(params, name)) throw new Error(`missing parameter ${name}`);
    return String(params[name]);
  });
}
export function planOperation(project, operation, params = {}, resolvedBindings = {}) {
  validateProject(project);
  if (!OPERATIONS.includes(operation)) throw new Error(`unknown operation: ${operation}`);
  if (params === null || typeof params !== 'object' || Array.isArray(params)) throw new Error('operation parameters must be an object');
  const service = operation.startsWith('git.') ? 'git' : operation.startsWith('task.') ? 'tasks' : 'codehost';
  const binding = project.services?.[service] ?? (service === 'git' ? { provider: 'git', transport: 'cli', command: 'git' } : undefined);
  if (!binding) throw new Error(`missing ${service} binding for ${operation}`);
  const write = WRITE_OPERATIONS.includes(operation);
  if (write && !project.permissions?.writeOperations?.includes(operation)) throw new Error(`write operation not enabled by project policy: ${operation}`);
  let mapping = binding.operations?.[operation];
  mapping ??= resolvedBindings?.[operation]?.mapping;
  if (!mapping && resolvedBindings?.[operation]?.error) throw new Error(resolvedBindings[operation].error);
  if (!mapping) throw new Error(`binding has no mapping for ${operation}`);
  const base = { operation, service, provider: binding.provider, transport: binding.transport, effect: write ? 'write' : 'read',
    authorization: write ? 'explicit-user-or-job-approval-required' : 'host-permissions-apply', availability: 'not-verified',
    output: operation.endsWith('.diff') ? 'save-to-artifact' : 'bounded-result' };
  if (binding.transport === 'mcp') return { ...base, tool: mapping.tool, arguments: substitute(mapping.arguments, params) };
  const args = substitute(mapping.args, params);
  if (args.some((arg) => !['string', 'number', 'boolean'].includes(typeof arg))) throw new Error('CLI arguments must be scalar values');
  return { ...base, command: binding.command, args: args.map(String), shell: false };
}
