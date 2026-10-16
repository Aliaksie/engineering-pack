import fs from 'node:fs';
import path from 'node:path';

const object = (value) => value !== null && typeof value === 'object' && !Array.isArray(value);
const namePattern = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
export function validateName(name, label, errors) {
  if (typeof name !== 'string' || !namePattern.test(name)) errors.push(`${label}: name must be non-empty kebab-case`);
}

export function readJson(file, errors) {
  try { return JSON.parse(fs.readFileSync(file, 'utf8')); }
  catch (error) { errors.push(`${file}: invalid JSON (${error.message})`); return null; }
}

export function readFrontmatter(file, errors) {
  let text;
  try { text = fs.readFileSync(file, 'utf8').replace(/\r\n/g, '\n'); }
  catch (error) { errors.push(`${file}: cannot read frontmatter (${error.message})`); return {}; }
  const header = /^---\n([^]*?)\n---(?:\n|$)/.exec(text);
  if (!header) {
    errors.push(`${file}: ${text.startsWith('---\n') ? 'unterminated' : 'missing'} YAML frontmatter`);
    return {};
  }
  const fields = Object.create(null);
  let sequence;
  for (const line of header[1].split('\n')) {
    const item = /^\s+-\s+(.+)$/.exec(line);
    if (sequence && item) { fields[sequence].push(item[1].trim()); continue; }
    if (/^\S/.test(line)) {
      sequence = undefined;
      const field = /^([A-Za-z][\w-]*):\s*(.*)$/.exec(line);
      if (!field) continue;
      fields[field[1]] = field[2];
      if (!field[2]) { sequence = field[1]; fields[sequence] = []; }
    }
  }
  return fields;
}

export function asList(value) {
  const values = Array.isArray(value) ? value : typeof value === 'string' ? value.replace(/^\[|\]$/g, '').split(',') : [];
  return values.filter((item) => typeof item === 'string').map((item) => item.trim()).filter(Boolean);
}

export function pluginDirectory(root, entry, errors = []) {
  const label = `plugin ${entry?.name ?? '<invalid>'}`;
  if (!object(entry)) { errors.push('marketplace: plugin entry must be an object'); return null; }
  if (typeof entry.source !== 'string' || !entry.source.startsWith('./')) {
    errors.push(`${label}: local source must start with ./`); return null;
  }
  const segments = entry.source.slice(2).split('/');
  if (segments.some((part) => !part || part === '.' || part === '..' || /[\\\0:]/.test(part))) {
    errors.push(`${label}: source escapes marketplace root or is not a canonical local path`); return null;
  }
  let current = path.resolve(root);
  try {
    for (const part of segments) {
      current = path.join(current, part);
      const stat = fs.lstatSync(current);
      if (stat.isSymbolicLink()) throw new Error('source contains a symbolic link');
      if (!stat.isDirectory()) throw new Error('source is not a directory');
    }
    return current;
  } catch (error) { errors.push(`${label}: source directory does not exist or is unsafe (${error.message})`); return null; }
}

export function pluginFiles(directory, errors = [], directories = []) {
  const files = [];
  const pending = [directory];
  let inspected = 0;
  while (pending.length) {
    const current = pending.pop();
    try {
      directories.push(current);
      for (const entry of fs.readdirSync(current, { withFileTypes: true })) {
        if (++inspected > 20000) { errors.push(`${directory}: file inspection limit exceeded`); return files; }
        const file = path.join(current, entry.name);
        if (entry.isSymbolicLink()) errors.push(`${file}: symbolic links are not accepted in plugin trees`);
        else if (entry.isDirectory()) pending.push(file);
        else if (entry.isFile()) files.push(file);
      }
    } catch (error) { errors.push(`${current}: cannot inspect directory (${error.message})`); }
  }
  return files.sort();
}

function advertised(term, content) {
  const escaped = term.toLowerCase().split('-').map((part) => part.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'));
  return new RegExp(escaped.join('[\\s_/-]*')).test(content);
}

export function validatePlugin(root, entry, errors) {
  const directory = pluginDirectory(root, entry, errors);
  if (!directory) return;
  const label = `plugin ${entry.name}`;
  validateName(entry.name, label, errors);
  const directories = [];
  const files = pluginFiles(directory, errors, directories);
  const manifestFile = path.join(directory, '.claude-plugin/plugin.json');
  if (!files.includes(manifestFile)) { errors.push(`${label}: missing .claude-plugin/plugin.json`); return; }
  const manifest = readJson(manifestFile, errors);
  if (!object(manifest)) { errors.push(`${label}: plugin manifest must be an object`); return; }
  validateName(manifest.name, `plugin manifest ${entry.name}`, errors);
  if (manifest.name !== entry.name) errors.push(`${label}: marketplace and manifest names differ`);
  if (typeof entry.version !== 'string' || !entry.version) errors.push(`${label}: marketplace entry has no version`);
  else if (entry.version !== manifest.version) errors.push(`${label}: marketplace and manifest versions differ`);
  if (typeof manifest.description !== 'string' || !manifest.description.trim()) errors.push(`${label}: manifest description is required by this repository`);

  const claims = [];
  for (const [kind, values] of [['tag', entry.tags], ['keyword', manifest.keywords]]) {
    if (values === undefined) continue;
    if (!Array.isArray(values)) { errors.push(`${label}: ${kind} declarations must be an array`); continue; }
    for (const term of values) {
      if (typeof term !== 'string' || !term.trim()) errors.push(`${label}: a ${kind} is empty or not a string`);
      else claims.push({ kind, term });
    }
  }
  if (claims.length) {
    const corpus = [];
    let bytes = 0;
    for (const file of files) {
      if (path.relative(directory, file).split(path.sep).includes('.claude-plugin')) continue;
      try {
        const size = fs.statSync(file).size;
        if (size > 4 * 1024 * 1024 || bytes + size > 32 * 1024 * 1024) throw new Error('advertising inspection byte limit exceeded');
        bytes += size;
        corpus.push(fs.readFileSync(file, 'utf8').toLowerCase());
      } catch (error) { errors.push(`${file}: cannot inspect advertised content (${error.message})`); }
    }
    const content = corpus.join('\n');
    for (const { kind, term } of claims) if (!advertised(term, content)) errors.push(`${label}: ${kind} "${term}" appears nowhere in ${entry.source}`);
  }
  const skillNames = directories.filter((file) => path.dirname(file) === path.join(directory, 'skills')).map((file) => path.basename(file));
  for (const name of skillNames) {
    validateName(name, `${label} skill ${name}`, errors);
    const skill = path.join(directory, 'skills', name, 'SKILL.md');
    if (!files.includes(skill)) { errors.push(`${label}: skill ${name} is missing SKILL.md`); continue; }
    if (!readFrontmatter(skill, errors).description) errors.push(`${label} skill ${name}: description is required`);
  }
  for (const file of files.filter((file) => path.dirname(file) === path.join(directory, 'agents') && file.endsWith('.md'))) {
    const fields = readFrontmatter(file, errors);
    validateName(fields.name, `${label} agent ${path.basename(file)}`, errors);
    if (!fields.description) errors.push(`${label} agent ${path.basename(file)}: description is required`);
    if (fields.permissionMode === 'bypassPermissions') errors.push(`${label} agent ${path.basename(file)}: bypassPermissions is prohibited`);
  }
}
