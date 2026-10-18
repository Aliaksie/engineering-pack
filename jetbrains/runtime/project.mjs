import fs from 'node:fs';
import path from 'node:path';

export const LAYERS = ['ui', 'backend', 'agent'];
export const LANGUAGES = ['javascript', 'typescript', 'python', 'java', 'kotlin', 'jvm', 'csharp', 'go'];
export const READ_OPERATIONS = ['git.status', 'git.diff', 'change.read', 'change.diff', 'task.read', 'task.search'];
export const WRITE_OPERATIONS = ['git.commit', 'git.push', 'change.publish', 'task.transition', 'task.comment'];
export const OPERATIONS = [...READ_OPERATIONS, ...WRITE_OPERATIONS];
const plain = (value) => value !== null && typeof value === 'object' && !Array.isArray(value);
export function relativeFile(value) {
  if (typeof value !== 'string' || !value || value.includes('\\') || value.startsWith('/') || /^[A-Za-z]:/.test(value)
    || value.split('/').some((part) => part === '..' || part === '') || value.includes('\0')) throw new Error(`expected project-relative path: ${String(value)}`);
  return path.posix.normalize(value);
}
function strings(value, field, allowed) {
  if (!Array.isArray(value) || value.some((entry) => typeof entry !== 'string' || (allowed && !allowed.includes(entry)))) throw new Error(`invalid ${field}`);
}
function keys(object, allowed, label) {
  if (!plain(object)) throw new Error(`${label} must be an object`);
  for (const key of Object.keys(object)) if (!allowed.includes(key)) throw new Error(`unknown ${label} field: ${key}`);
}
export function validateProject(project) {
  keys(project, ['formatVersion', 'languages', 'layers', 'runtimes', 'frameworks', 'providers', 'modules', 'services', 'execution', 'permissions', 'checkpoint', 'review', 'routing'], 'project');
  if (project.formatVersion !== 1) throw new Error('unsupported project formatVersion');
  for (const [field, allowed] of [['languages', LANGUAGES], ['layers', LAYERS], ['runtimes'], ['frameworks'], ['providers']]) {
    if (project[field] !== undefined) strings(project[field], field, allowed);
  }
  if (project.modules !== undefined && !Array.isArray(project.modules)) throw new Error('modules must be an array');
  const names = new Set();
  for (const module of project.modules ?? []) {
    keys(module, ['name', 'path', 'owner', 'languages', 'layers', 'skills', 'dependsOn', 'checks', 'providers', 'runtimes', 'frameworks'], 'module');
    if (typeof module.name !== 'string' || !/^[a-z0-9-]+$/.test(module.name) || names.has(module.name)) throw new Error('invalid or duplicate module name');
    names.add(module.name); relativeFile(module.path);
    for (const [field, allowed] of [['languages', LANGUAGES], ['layers', LAYERS], ['skills'], ['dependsOn'], ['providers'], ['runtimes'], ['frameworks']]) {
      if (module[field] !== undefined) strings(module[field], `module.${field}`, allowed);
    }
    if (module.owner !== undefined && typeof module.owner !== 'string') throw new Error('module owner must be text');
    if (module.checks !== undefined && !Array.isArray(module.checks)) throw new Error('checks must be an array');
    for (const check of module.checks ?? []) {
      keys(check, ['command', 'args'], 'check');
      if (typeof check.command !== 'string' || !check.command.trim()) throw new Error('check command required');
      strings(check.args ?? [], 'check args');
    }
  }
  for (const module of project.modules ?? []) for (const dependency of module.dependsOn ?? []) {
    if (!names.has(dependency)) throw new Error(`unknown module dependency: ${dependency}`);
  }
  if (project.services !== undefined) keys(project.services, ['git', 'codehost', 'tasks'], 'services');
  for (const [service, binding] of Object.entries(project.services ?? {})) {
    keys(binding, ['provider', 'transport', 'command', 'host', 'repository', 'operations'], 'binding');
    if (typeof binding.provider !== 'string' || !binding.provider || !['cli', 'mcp'].includes(binding.transport)) throw new Error('invalid service binding');
    if (binding.transport === 'cli' && (typeof binding.command !== 'string' || !binding.command.trim())) throw new Error('binding command required');
    if (binding.host !== undefined && !/^[A-Za-z0-9.-]+(?::[0-9]+)?$/.test(binding.host)) throw new Error('invalid binding host');
    if (binding.repository !== undefined && (!/^[A-Za-z0-9_.-]+(?:\/[A-Za-z0-9_.-]+)+$/.test(binding.repository) || binding.repository.split('/').some((part) => part === '.' || part === '..'))) throw new Error('invalid repository identity');
    for (const field of ['host', 'repository']) if (binding[field] !== undefined && (typeof binding[field] !== 'string' || !binding[field].trim())) throw new Error(`invalid binding ${field}`);
    if (binding.operations !== undefined && !plain(binding.operations)) throw new Error('binding operations must be an object');
    for (const [operation, mapping] of Object.entries(binding.operations ?? {})) {
      if (!operation.startsWith(service === 'tasks' ? 'task.' : service === 'codehost' ? 'change.' : 'git.')) throw new Error('operation belongs to a different service');
      if (!OPERATIONS.includes(operation)) throw new Error(`unknown operation ${operation}`);
      keys(mapping, binding.transport === 'cli' ? ['args'] : ['tool', 'arguments'], 'operation mapping');
      if (binding.transport === 'cli') strings(mapping.args, 'operation args');
      else if (typeof mapping.tool !== 'string' || !/^[\w.:-]+$/.test(mapping.tool) || !plain(mapping.arguments)) throw new Error('MCP mapping requires exact tool name and arguments');
    }
  }
  if (project.permissions !== undefined) {
    keys(project.permissions, ['writeOperations'], 'permissions');
    strings(project.permissions.writeOperations ?? [], 'writeOperations', WRITE_OPERATIONS);
  }
  if (project.execution !== undefined) {
    keys(project.execution, ['roles', 'hosts'], 'execution');
    if (project.execution.roles !== undefined && !plain(project.execution.roles)) throw new Error('execution roles must be an object');
    for (const [role, rule] of Object.entries(project.execution.roles ?? {})) {
      if (!['locator', 'researcher', 'implementer', 'reviewer', 'planner'].includes(role)) throw new Error('unknown execution role');
      keys(rule, ['mode', 'profile'], 'execution role');
      if (!['main', 'delegate', 'tool'].includes(rule.mode) || (rule.profile !== undefined && typeof rule.profile !== 'string')) throw new Error('invalid execution rule');
    }
    if (project.execution.hosts !== undefined && !plain(project.execution.hosts)) throw new Error('execution hosts must be an object');
    for (const host of Object.values(project.execution.hosts ?? {})) {
      keys(host, ['delegation', 'models'], 'execution host');
      if (host.delegation !== undefined && typeof host.delegation !== 'boolean') throw new Error('delegation must be boolean');
      if (host.models !== undefined && (!plain(host.models) || Object.values(host.models).some((model) => typeof model !== 'string' || !model))) throw new Error('model profiles must map to explicit host model names');
    }
  }
  if (project.checkpoint !== undefined) {
    keys(project.checkpoint, ['enabled'], 'checkpoint');
    if (typeof project.checkpoint.enabled !== 'boolean') throw new Error('checkpoint enabled must be boolean');
  }
  if (project.review !== undefined) {
    keys(project.review, ['readOnly'], 'review');
    if (typeof project.review.readOnly !== 'boolean') throw new Error('review.readOnly must be boolean');
  }
  if (project.routing !== undefined) {
    keys(project.routing, ['editHints'], 'routing');
    if (typeof project.routing.editHints !== 'boolean') throw new Error('routing editHints must be boolean');
  }
  return project;
}

export function readProject(root) {
  const file = path.join(root, '.capabilities', 'project.json');
  for (const candidate of [path.dirname(file), file]) {
    const stat = fs.lstatSync(candidate, { throwIfNoEntry: false });
    if (stat?.isSymbolicLink()) throw new Error('project configuration may not be a symlink');
  }
  if (!fs.existsSync(file)) return { formatVersion: 1 };
  return validateProject(JSON.parse(fs.readFileSync(file, 'utf8')));
}
