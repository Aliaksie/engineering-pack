import { LANGUAGES, LAYERS, relativeFile } from './project.mjs';

const slug = '[a-z0-9]+(?:-[a-z0-9]+)*';
const identity = new RegExp(`^(workflow|practice|language|runtime|framework|provider|layer):${slug}$`);
const skill = new RegExp(`^${slug}:${slug}$`);
const reference = new RegExp(`^${slug}:${slug}/references/(?:${slug}/)*${slug}\\.md$`);
export const VALUE_FIELDS = ['problem', 'contribution', 'inputs', 'result', 'boundaries', 'consumer', 'evidence'];
const dimensions = ['intents', 'risks', 'languages', 'runtimes', 'frameworks', 'providers', 'layers', 'paths'];
const object = (value) => value !== null && typeof value === 'object' && !Array.isArray(value);
function keys(value, allowed, label) {
  if (!object(value)) throw new Error(`${label} must be an object`);
  for (const key of Object.keys(value)) if (!allowed.includes(key)) throw new Error(`unknown ${label} field: ${key}`);
}
function text(value, label) {
  if (typeof value !== 'string' || !value.trim()) throw new Error(`${label} must be nonempty text`);
}
function strings(value, label, { empty = false, allowed, pattern } = {}) {
  if (!Array.isArray(value) || (!empty && !value.length)) throw new Error(`${label} must be ${empty ? 'an' : 'a nonempty'} array`);
  for (const item of value) {
    text(item, label);
    if ((allowed && !allowed.includes(item)) || (pattern && !pattern.test(item))) throw new Error(`invalid ${label}: ${item}`);
  }
  if (new Set(value).size !== value.length) throw new Error(`duplicate ${label}`);
}

// Shape validation is portable. Authoring passes availableTargets for source-level
// resolution; an installed optional package may legitimately lack other packages.
export function validatePlaybookCatalog(catalog, { availableTargets } = {}) {
  keys(catalog, ['formatVersion', 'intents', 'stackIntents', 'extensions', 'layerExtensions', 'runtimeImplications', 'entries'], 'catalog');
  if (catalog.formatVersion !== 1) throw new Error('unsupported catalog formatVersion');
  strings(catalog.intents, 'intents', { pattern: new RegExp(`^${slug}$`) });
  strings(catalog.stackIntents, 'stackIntents', { allowed: catalog.intents });
  for (const [field, allowed] of [['extensions', LANGUAGES], ['layerExtensions', LAYERS], ['runtimeImplications']]) {
    const mapping = catalog[field];
    if (!object(mapping)) throw new Error(`${field} must be an object`);
    for (const [key, value] of Object.entries(mapping)) {
      text(value, field);
      if (field === 'runtimeImplications' ? !LANGUAGES.includes(key) : !/^\.[a-z0-9]+$/.test(key)) throw new Error(`invalid ${field} key: ${key}`);
      if (allowed ? !allowed.includes(value) : !new RegExp(`^${slug}$`).test(value)) throw new Error(`invalid ${field} value: ${value}`);
    }
  }
  if (!Array.isArray(catalog.entries) || !catalog.entries.length) throw new Error('entries must be a nonempty array');
  const ids = new Set();
  for (const entry of catalog.entries) {
    keys(entry, ['id', 'skills', 'fallbackSkills', 'references', 'when', 'value', 'unavailable', 'documentationQuestion', 'contextOnly'], 'entry');
    if (!identity.test(entry.id) || ids.has(entry.id)) throw new Error(`invalid or duplicate logical id: ${entry.id}`);
    ids.add(entry.id);
    strings(entry.skills, `${entry.id} skills`, { empty: true, pattern: skill });
    if (entry.fallbackSkills !== undefined) {
      if (!object(entry.fallbackSkills)) throw new Error(`${entry.id} fallbackSkills must be an object`);
      for (const [primary, fallback] of Object.entries(entry.fallbackSkills)) {
        if (!entry.skills.includes(primary) || !skill.test(fallback) || primary === fallback) throw new Error(`${entry.id} invalid skill fallback`);
      }
    }
    strings(entry.references, `${entry.id} references`, { empty: true, pattern: reference });
    keys(entry.when, dimensions, `${entry.id} selector`);
    if (!Object.keys(entry.when).length) throw new Error(`${entry.id} requires selectors`);
    for (const [dimension, values] of Object.entries(entry.when)) {
      strings(values, `${entry.id} ${dimension}`, {
        allowed: dimension === 'intents' ? catalog.intents : dimension === 'languages' ? LANGUAGES : dimension === 'layers' ? LAYERS : undefined,
        pattern: dimension === 'paths' ? undefined : new RegExp(`^${slug}$`)
      });
      if (dimension === 'paths') for (const value of values) {
        if (relativeFile(value) !== value || /[*?\[\]{}]/.test(value)) throw new Error(`invalid selector path: ${value}`);
      }
    }
    if (entry.id.startsWith('workflow:') && (!entry.when.intents || Object.keys(entry.when).length !== 1)) throw new Error(`${entry.id} workflow requires only an intent selector`);
    if (entry.when.intents?.some((intent) => !catalog.stackIntents.includes(intent)) && !/^(workflow|practice):/.test(entry.id)) throw new Error(`${entry.id} specialization cannot select non-stack intents`);
    keys(entry.value, VALUE_FIELDS, `${entry.id} value`);
    for (const field of VALUE_FIELDS) text(entry.value[field], `${entry.id} value.${field}`);
    for (const field of ['unavailable', 'documentationQuestion']) if (entry[field] !== undefined) text(entry[field], `${entry.id} ${field}`);
    if (entry.contextOnly !== undefined && typeof entry.contextOnly !== 'boolean') throw new Error(`${entry.id} contextOnly must be boolean`);
    if (!entry.skills.length && !entry.references.length && !entry.unavailable && entry.contextOnly !== true) throw new Error(`${entry.id} requires load targets, unavailable guidance or explicit contextOnly`);
    if (availableTargets) for (const target of [...entry.skills, ...Object.values(entry.fallbackSkills ?? {}), ...entry.references]) {
      if (!availableTargets.has(target)) throw new Error(`unresolved catalog target: ${target}`);
    }
  }
  for (const runtime of Object.values(catalog.runtimeImplications)) if (!ids.has(`runtime:${runtime}`)) throw new Error(`unresolved implied runtime: ${runtime}`);
  return catalog;
}
