import { OPERATIONS, WRITE_OPERATIONS, validateProject } from '../contracts/project.mjs';

function names(value, label) {
  if (value === undefined) return undefined;
  if (!Array.isArray(value) || value.some((name) => typeof name !== 'string' || !name.trim())) {
    throw new Error(`${label} must be an array of tool or command names`);
  }
  return new Set(value);
}

function parameters(value, result = new Set()) {
  if (Array.isArray(value)) for (const entry of value) parameters(entry, result);
  else if (value && typeof value === 'object') for (const entry of Object.values(value)) parameters(entry, result);
  else if (typeof value === 'string') {
    for (const match of value.matchAll(/\{([a-zA-Z][\w]*)\}/g)) result.add(match[1]);
  }
  return [...result].sort();
}

// A session-level metadata summary, not a tool discovery request or authorization check.
export function prepareServices(project, { availableCommands, availableMcpTools, bindings = {} } = {}) {
  validateProject(project);
  const observed = { cli: names(availableCommands, 'availableCommands'), mcp: names(availableMcpTools, 'availableMcpTools') };
  const services = ['git', 'codehost', 'tasks'].map((service) => {
    const explicit = project.services?.[service];
    const binding = explicit ?? (service === 'git' ? { provider: 'git', transport: 'cli', command: 'git' } : undefined);
    const prefix = service === 'codehost' ? 'change.' : service === 'tasks' ? 'task.' : 'git.';
    const operations = OPERATIONS.filter((operation) => operation.startsWith(prefix)).map((operation) => {
      const write = WRITE_OPERATIONS.includes(operation);
      const base = { operation, effect: write ? 'write' : 'read',
        projectPolicy: write ? (project.permissions?.writeOperations?.includes(operation) ? 'enabled' : 'disabled') : 'read',
        authorization: write ? 'explicit-user-or-job-approval-required' : 'host-permissions-apply' };
      if (!binding) return { ...base, mapping: 'missing', availability: 'not-applicable', missing: `missing ${service} binding` };
      const mapping = binding.operations?.[operation];
      const built = mapping ? undefined : bindings[operation];
      if (!mapping && (!built || built.missing)) return { ...base, mapping: 'missing', availability: 'not-applicable', missing: built?.missing ?? `binding has no mapping for ${operation}` };
      const name = binding.transport === 'cli' ? binding.command : mapping.tool;
      const inventory = observed[binding.transport];
      return { ...base, mapping: mapping ? 'configured' : 'builtin',
        ...(binding.transport === 'cli' ? { command: name } : { tool: name }),
        ...(mapping ? { requiredParameters: parameters(binding.transport === 'cli' ? mapping.args : mapping.arguments) } : { requiredParameters: built.requiredParameters, ...(built.optionalParameters ? { optionalParameters: built.optionalParameters } : {}) }),
        availability: inventory === undefined ? 'unverified' : inventory.has(name) ? 'observed-available' : 'not-observed' };
    });
    return { service, configured: Boolean(binding), ...(binding ? {
      source: explicit ? 'project' : 'local-default', provider: binding.provider, transport: binding.transport,
      scope: { ...(binding.host ? { host: binding.host } : {}), ...(binding.repository ? { repository: binding.repository } : {}), account: 'unverified' }
    } : {}), operations };
  });
  return { services, reuse: {
    prepare: 'once-per-session-or-first-service-need',
    refreshOn: ['project-binding-change', 'tool-connection-change', 'account-or-scope-change', 'relevant-operation-failure'],
    limits: 'Reuse metadata between turns. Tool names do not establish account identity, authorization, or fresh remote state.'
  } };
}
