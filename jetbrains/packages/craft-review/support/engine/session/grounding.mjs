import { validateTaskContext } from './acceptance.mjs';
import { matchingModules } from '../routing/route.mjs';
import { relativeFile, validateProject } from '../contracts/project.mjs';
import { privatePath } from '../contracts/paths.mjs';
import { digest } from '../cache/identity.mjs';

const local = source => !['external', 'request'].includes(source.kind);
const member = (file, scope) => scope === '.' || file === scope || file.startsWith(`${scope}/`);
const overlaps = (entry, scope) => entry.scope.some(area => scope.some(file => member(file, area) || member(area, file)));
export function groundingRequest({ scope, sources = [] }) {
  if (!Array.isArray(scope) || !scope.length || scope.length > 32 || !Array.isArray(sources) || sources.length > 24) throw new Error('grounding:capacity');
  scope = [...new Set(scope.map(relativeFile))];
  const files = new Set(['.craft/project.json']);
  for (const file of scope) {
    if (privatePath(file)) throw new Error('grounding:private-source');
    files.add('AGENTS.md');
    const parts = file.split('/');
    // Scope may be a directory; checking its AGENTS.md is harmless for file scopes.
    for (let i = 1; i <= parts.length; i++) if (file !== '.') files.add(`${parts.slice(0, i).join('/')}/AGENTS.md`);
  }
  for (const source of sources) if (local(source) && overlaps(source, scope)) {
    const file = relativeFile(source.reference);
    if (privatePath(file)) throw new Error('grounding:private-source');
    files.add(file);
  }
  if (files.size > 64) throw new Error('grounding:capacity');
  return { scope, files: [...files] };
}

// Only explicit classifications become constraints. Literal lookup matches never do.
export function composeTaskContext({ scope, project, observations, sources = [], constraints = [], checks = [], questions = [], observedAt, binding, discovery }) {
  validateProject(project);
  scope = groundingRequest({ scope, sources }).scope;
  const packet = { formatVersion: 1, scope, sources: [...sources], constraints, checks: [...checks], questions: [...questions], coverage: 'unknown' };
  validateTaskContext(packet);
  const selected = matchingModules(project, scope);
  const relevant = entry => overlaps(entry, scope);
  packet.sources = packet.sources.filter(relevant);
  const known = new Set(packet.sources.map(entry => entry.id));
  packet.constraints = constraints.filter(relevant);
  packet.checks = checks.filter(relevant);
  if ([...packet.constraints, ...packet.checks].some(entry => entry.sourceIds.some(id => !known.has(id)))) throw new Error('grounding:basis-outside-scope');
  // Keep conflicts intact: removing one side must not silently resolve a disagreement.
  if (packet.constraints.some(entry => entry.conflicts.some(id => !packet.constraints.some(other => other.id === id)))) throw new Error('grounding:conflict-outside-scope');
  const addQuestion = text => { if (!packet.questions.includes(text)) packet.questions.push(text); };
  for (const observation of observations) {
    if (observation.status === 'unavailable') addQuestion(`Source unavailable: ${observation.reference}`);
    if (observation.status !== 'observed') continue;
    const existing = packet.sources.filter(entry => entry.reference === observation.reference && local(entry));
    if (existing.length) {
      for (const entry of existing) if (entry.identity !== observation.identity) { entry.identity = null; entry.status = 'unknown'; addQuestion(`Source changed: ${entry.reference}`); }
    } else if (observation.reference.endsWith('AGENTS.md') || observation.reference === '.craft/project.json') {
      packet.sources.push({ id: `local-${digest(observation.reference).slice(0, 16)}`, kind: observation.reference === '.craft/project.json' ? 'configuration' : 'instruction',
        reference: observation.reference, identity: observation.identity, observedAt, scope: [observation.reference.includes('/') && observation.reference !== '.craft/project.json' ? observation.reference.slice(0, observation.reference.lastIndexOf('/')) : '.'], provenance: 'observed', status: 'unknown' });
    }
  }
  for (const source of packet.sources) {
    if (local(source) && !observations.some(entry => entry.reference === source.reference && entry.status === 'observed')) {
      source.identity = null; source.status = 'unknown'; addQuestion(`Source unavailable: ${source.reference}`);
    }
  }
  for (const constraint of packet.constraints) if (constraint.status === 'accepted' && constraint.sourceIds.some(id => packet.sources.some(source => source.id === id && (source.identity === null || source.status !== 'accepted')))) constraint.status = 'unknown';
  const configuration = packet.sources.find(entry => entry.reference === '.craft/project.json');
  if (configuration) for (const module of selected) {
    packet.constraints.push({ id: `module-${module.name}`, description: JSON.stringify({ module: module.name, owner: module.owner ?? null, dependsOn: module.dependsOn ?? [] }), sourceIds: [configuration.id], authority: 'project', status: configuration.identity ? 'accepted' : 'unknown', scope: [module.path], required: false, conflicts: [] });
    for (const check of module.checks ?? []) {
      packet.checks.push({ id: `declared-${digest([module.name, check]).slice(0, 16)}`, description: JSON.stringify({ cwd: module.path, command: check.command, args: check.args ?? [] }), sourceIds: [configuration.id], scope: [module.path] });
    }
  }
  const seen = new Set();
  packet.checks = packet.checks.filter(entry => { const key = JSON.stringify(entry); if (seen.has(key)) return false; seen.add(key); return true; });
  if (!binding?.identity) addQuestion('Repository identity unavailable; source freshness is unknown.');
  if (discovery && (discovery.truncated || Object.values(discovery.skipped ?? {}).some(Boolean))) addQuestion('Discovery coverage is incomplete; matches do not prove absence.');
  if (!packet.sources.length) addQuestion('No grounded sources supplied or observed.');
  packet.coverage = packet.questions.length ? 'incomplete' : 'unknown';
  return validateTaskContext(packet);
}
