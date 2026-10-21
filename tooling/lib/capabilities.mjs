import fs from 'node:fs';
import path from 'node:path';

const NAME = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
const VERSION = /^\d+\.\d+\.\d+(?:-[a-z0-9.-]+)?$/;
const ARRAYS = ['skills', 'workflows', 'standards', 'evals', 'tools', 'hooks', 'mcp', 'optionalDependencies'];
const KEYS = ['formatVersion', 'name', 'version', 'description', 'author', 'instructions', ...ARRAYS];
export const json = (file) => JSON.parse(fs.readFileSync(file, 'utf8'));
export const stringify = (value) => `${JSON.stringify(value, null, 2)}\n`;

export function files(dir) {
  if (!fs.existsSync(dir)) return [];
  return fs.readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))
    .flatMap((entry) => {
      const full = path.join(dir, entry.name);
      if (/(^|\/)\.capabilities\/state(?:\/|$)|(^|\/)\.git(?:\/|$)/.test(full.split(path.sep).join('/'))) return [];
      if (entry.isSymbolicLink()) throw new Error(`symlinks are not distributable: ${full}`);
      return entry.isDirectory() ? files(full) : [full];
    });
}

export function localPath(root, value, directory = false) {
  if (typeof value !== 'string' || !value || value.includes('\\') || path.isAbsolute(value)
      || value.split('/').some((part) => !part || part === '..' || part === '.')) {
    throw new Error(`invalid package path: ${String(value)}`);
  }
  const target = path.resolve(root, value);
  if (/(^|\/)\.capabilities\/state(?:\/|$)/.test(value)) throw new Error('private workspace is not package content');
  // Reject symlinks at every component, not only the final file.
  let current = root;
  for (const part of value.split('/')) {
    current = path.join(current, part);
    if (fs.lstatSync(current).isSymbolicLink()) throw new Error(`symlink in package path: ${value}`);
  }
  const stat = fs.statSync(target);
  if (directory ? !stat.isDirectory() : !stat.isFile()) throw new Error(`wrong path kind: ${value}`);
  return target;
}

export function frontmatter(text) {
  const match = /^---\n([\s\S]*?)\n---\n([\s\S]*)$/.exec(text);
  if (!match) throw new Error('missing or malformed frontmatter');
  const fields = {};
  for (const line of match[1].split('\n')) {
    if (!line.trim() || line.trim().startsWith('#')) continue;
    const field = /^([a-z-]+):\s*(.+)$/.exec(line);
    if (!field || !['name', 'description'].includes(field[1])) throw new Error(`nonportable frontmatter: ${line}`);
    if (field[1] in fields) throw new Error(`duplicate frontmatter: ${field[1]}`);
    fields[field[1]] = field[2].startsWith('"') ? JSON.parse(field[2]) : field[2];
  }
  if (!NAME.test(fields.name) || fields.name.length > 64 || !fields.description) throw new Error('name/description required');
  return { ...fields, body: match[2] };
}

function unique(values, label) {
  if (new Set(values).size !== values.length) throw new Error(`duplicate ${label}`);
}

export function validateCase(testCase) {
  if (!NAME.test(testCase.id) || typeof testCase.prompt !== 'string' || !testCase.prompt.trim()
      || !Array.isArray(testCase.criteria) || !testCase.criteria.length) throw new Error('invalid eval case');
  unique(testCase.criteria.map((criterion) => criterion.id), 'criterion');
  for (const criterion of testCase.criteria) {
    if (!NAME.test(criterion.id) || typeof criterion.description !== 'string' || !criterion.description.trim()) throw new Error('invalid eval criterion');
  }
}

export function validateCapability(root, manifest) {
  const errors = [];
  const check = (fn) => { try { fn(); } catch (error) { errors.push(error.message); } };
  check(() => {
    for (const key of Object.keys(manifest)) if (!KEYS.includes(key)) throw new Error(`unknown manifest field: ${key}`);
    if (manifest.formatVersion !== 1) throw new Error('unsupported formatVersion');
    if (manifest.instructions !== 'AGENTS.md') throw new Error('instructions must be AGENTS.md');
    if (!NAME.test(manifest.name) || manifest.name.length > 40 || !VERSION.test(manifest.version)) throw new Error('invalid package name/version');
    if (typeof manifest.description !== 'string' || !manifest.description.trim()) throw new Error('description required');
    if (typeof manifest.author?.name !== 'string' || !manifest.author.name.trim() || Object.keys(manifest.author).some((key) => key !== 'name')) throw new Error('author.name required');
    for (const key of ARRAYS) if (!Array.isArray(manifest[key])) throw new Error(`${key} must be an array`);
  });
  if (errors.length) return errors;
  check(() => localPath(root, manifest.instructions));
  for (const key of ['skills', 'workflows', 'standards', 'evals']) {
    check(() => unique(manifest[key], key));
    for (const entry of manifest[key]) check(() => {
      const full = localPath(root, entry, key === 'skills');
      if (key === 'skills' || key === 'workflows') {
        const info = frontmatter(fs.readFileSync(key === 'skills' ? path.join(full, 'SKILL.md') : full, 'utf8'));
        if (key === 'skills' && info.name !== path.basename(entry)) throw new Error(`skill folder/name mismatch: ${entry}`);
        if (key === 'workflows' && info.name !== path.basename(entry, '.md')) throw new Error(`workflow file/name mismatch: ${entry}`);
      }
      if (key === 'evals') validateCase(json(full));
    });
  }
  check(() => {
    const skillIds = manifest.skills.map((entry) => path.basename(entry));
    const workflowIds = manifest.workflows.map((entry) => `workflow-${path.basename(entry, '.md')}`);
    unique([...skillIds, ...workflowIds], 'exported component');
    if ([...skillIds, ...workflowIds].some((name) => `${manifest.name}-${name}`.length > 64)) throw new Error('exported skill name exceeds 64 characters');
    unique(manifest.evals.map((entry) => json(localPath(root, entry)).id), 'eval case id');
  });
  const registeredSkills = manifest.skills.filter((entry) => typeof entry === 'string').map((entry) => path.basename(entry)).sort();
  check(() => {
    const dir = path.join(root, 'skills');
    const actual = fs.existsSync(dir) ? fs.readdirSync(dir).sort() : [];
    if (JSON.stringify(actual) !== JSON.stringify(registeredSkills)) throw new Error('skills must declare exactly the skill directories on disk');
  });
  for (const key of ['tools', 'hooks', 'mcp']) check(() => unique(manifest[key].map((entry) => entry.name), key));
  for (const tool of manifest.tools) check(() => {
    for (const key of Object.keys(tool)) if (!['name', 'path', 'runtime', 'network'].includes(key)) throw new Error(`unknown tool field: ${key}`);
    if (!NAME.test(tool.name) || !['node', 'bash'].includes(tool.runtime) || !['none', 'read', 'write'].includes(tool.network)) throw new Error('invalid tool declaration');
    localPath(root, tool.path);
  });
  for (const hook of manifest.hooks) check(() => {
    for (const key of Object.keys(hook)) if (!['name', 'event', 'tool', 'handler'].includes(key)) throw new Error(`unknown hook field: ${key}`);
    if (!NAME.test(hook.name) || !['session-start', 'before-prompt', 'before-tool', 'after-tool', 'before-compact', 'after-compact', 'turn-end'].includes(hook.event) || !['shell', 'any'].includes(hook.tool) || !hook.handler.endsWith('.mjs')) throw new Error('unsupported portable hook contract');
    localPath(root, hook.handler);
  });
  for (const server of manifest.mcp) check(() => {
    const allowed = server.transport === 'http' ? ['name', 'transport', 'url'] : ['name', 'transport', 'command', 'args', 'env'];
    for (const key of Object.keys(server)) if (!allowed.includes(key)) throw new Error(`unsupported MCP field: ${key}`);
    if (!NAME.test(server.name) || !['http', 'stdio'].includes(server.transport)) throw new Error('invalid MCP server');
    if (server.transport === 'http') {
      const url = new URL(server.url);
      if (url.protocol !== 'https:' || url.username || url.password || url.hash || url.search) throw new Error('MCP requires HTTPS without credentials, fragments or query secrets');
    } else {
      if (typeof server.command !== 'string' || !server.command.trim()) throw new Error('MCP command required');
      if (server.args !== undefined && (!Array.isArray(server.args) || server.args.some((arg) => typeof arg !== 'string'))) throw new Error('MCP args must be strings');
      if (server.env !== undefined && (server.env === null || typeof server.env !== 'object' || Array.isArray(server.env))) throw new Error('invalid MCP environment');
      for (const [key, value] of Object.entries(server.env ?? {})) {
        if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(key) || typeof value !== 'string' || !/^\$\{[A-Z_][A-Z0-9_]*\}$/.test(value)) throw new Error('MCP env values must reference external variables, never literals');
      }
    }
  });
  check(() => {
    unique(manifest.optionalDependencies, 'optional dependency');
    for (const name of manifest.optionalDependencies) if (!NAME.test(name) || name === manifest.name) throw new Error('invalid optional dependency');
    files(root);
  });
  return errors;
}

export function loadCapabilities(repo) {
  const directory = path.join(repo, 'packages');
  const result = fs.readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name)).map((entry) => {
    if (!entry.isDirectory()) throw new Error(`unexpected entry under packages/: ${entry.name}`);
    const root = path.join(directory, entry.name);
    const manifest = json(path.join(root, 'capability.json'));
    const errors = validateCapability(root, manifest);
    if (manifest.name !== entry.name) errors.push('package directory/name mismatch');
    if (errors.length) throw new Error(`${entry.name}: ${errors.join('; ')}`);
    return { root, manifest };
  });
  const names = new Set(result.map(({ manifest }) => manifest.name));
  for (const { manifest } of result) for (const dependency of manifest.optionalDependencies) {
    if (!names.has(dependency)) throw new Error(`${manifest.name}: unknown optional dependency ${dependency}`);
  }
  const exportedNames = result.flatMap(({ manifest }) => [...manifest.skills.map((entry) => path.basename(entry)), ...manifest.workflows.map((entry) => `workflow-${path.basename(entry, '.md')}`)].map((name) => `${manifest.name}-${name}`));
  unique(exportedNames, 'namespaced skill name');
  return result;
}
