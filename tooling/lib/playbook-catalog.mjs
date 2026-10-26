import fs from 'node:fs';
import path from 'node:path';
import { json, localPath } from './capabilities.mjs';

// The catalog shape is validated where `craft` embeds it; authoring resolves its load targets.
export function validateCatalogSources(repo, packages) {
  const catalog = json(path.join(repo, 'engine/routing/catalog.json'));
  const byName = new Map(packages.map((entry) => [entry.manifest.name, entry]));
  for (const entry of catalog.entries) for (const target of [...entry.skills, ...Object.values(entry.fallbackSkills ?? {}), ...entry.references]) {
    const [name, local] = target.split(':');
    const pkg = byName.get(name);
    const folder = `skills/${local.split('/')[0]}`;
    const workflow = !local.includes('/') && local.startsWith('workflow-') ? `workflows/${local.slice('workflow-'.length)}.md` : null;
    const registeredWorkflow = workflow && pkg?.manifest.workflows.includes(workflow);
    if (!registeredWorkflow && !pkg?.manifest.skills.includes(folder)) throw new Error(`unresolved catalog target: ${target}`);
    const source = registeredWorkflow ? workflow : `skills/${local}${target.includes('/references/') ? '' : '/SKILL.md'}`;
    const file = localPath(pkg.root, source);
    if (!fs.readFileSync(file, 'utf8').trim()) throw new Error(`empty catalog target: ${target}`);
  }
  return catalog;
}
