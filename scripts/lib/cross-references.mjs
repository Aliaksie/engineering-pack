import fs from 'node:fs';
import path from 'node:path';
import { asList, pluginDirectory, pluginFiles, readFrontmatter } from './plugin-manifest.mjs';

const qualified = /`([a-z0-9]+(?:-[a-z0-9]+)*):([a-z0-9]+(?:-[a-z0-9]+)*)`/g;
const bare = /`([a-z0-9]+(?:-[a-z0-9]+)*)`/g;

export function validateReferences(root, catalog, errors) {
  const components = new Map();
  for (const entry of Array.isArray(catalog?.plugins) ? catalog.plugins : []) {
    const directory = pluginDirectory(root, entry, errors);
    if (!directory || typeof entry.name !== 'string') continue;
    const files = pluginFiles(directory, errors);
    const skills = new Set();
    const other = new Set();
    for (const file of files) {
      const parts = path.relative(directory, file).split(path.sep);
      if (parts.length === 3 && parts[0] === 'skills' && parts[2] === 'SKILL.md') skills.add(parts[1]);
      if (parts.length === 2 && ['agents', 'commands'].includes(parts[0]) && file.endsWith('.md')) other.add(path.basename(file, '.md'));
    }
    components.set(entry.name, { directory, files: files.filter((file) => file.endsWith('.md')), skills, other });
  }
  function resolveSkill(citation) {
    const parts = citation.split(':');
    if (parts.length === 2) return components.get(parts[0])?.skills.has(parts[1]) ? citation : null;
    if (parts.length !== 1) return null;
    for (const [owner, entry] of components) if (entry.skills.has(citation)) return `${owner}:${citation}`;
    return null;
  }
  for (const [plugin, entry] of components) for (const file of entry.files) {
    let content;
    try { content = fs.readFileSync(file, 'utf8').replace(/\r\n/g, '\n'); }
    catch (error) { errors.push(`${file}: cannot inspect references (${error.message})`); continue; }
    const label = path.relative(root, file);
    for (const [, owner, name] of content.matchAll(qualified)) {
      const target = components.get(owner);
      if (target && !target.skills.has(name) && !target.other.has(name)) errors.push(`${label}: cites ${owner}:${name}, which ${owner} does not ship as a skill, agent, or command`);
    }
    if (path.dirname(file) !== path.join(entry.directory, 'agents')) continue;
    const fields = readFrontmatter(file, errors);
    const preloads = new Set();
    for (const citation of asList(fields.skills)) {
      const id = resolveSkill(citation);
      if (id) preloads.add(id);
      else errors.push(`plugin ${plugin} agent ${path.basename(file)}: preloads skill \`${citation}\`, which no plugin in this marketplace ships`);
    }
    const allowed = asList(fields.tools);
    const denied = asList(fields.disallowedTools);
    if ((!allowed.length || allowed.includes('Skill')) && !denied.includes('Skill')) continue;
    const body = content.replace(/^---\n[^]*?\n---(?:\n|$)/, '');
    const needed = new Set();
    for (const [, owner, name] of body.matchAll(qualified)) {
      const id = resolveSkill(`${owner}:${name}`);
      if (id) needed.add(id);
    }
    for (const [, citation] of body.matchAll(bare)) {
      const id = resolveSkill(citation);
      if (id) needed.add(id);
    }
    const unavailable = [...needed].filter((id) => !preloads.has(id));
    if (unavailable.length) errors.push(`plugin ${plugin} agent ${path.basename(file)}: names skill(s) ${unavailable.join(', ')} but cannot load them; preload via \`skills:\` or grant \`Skill\``);
  }
  return errors;
}
