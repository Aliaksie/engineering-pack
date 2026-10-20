import { DEFAULT_MODEL_PROFILES, DEFAULT_MODEL_ROLES } from './model-profiles.mjs';
// Preferences select behavior; authorization and host support remain separate facts.
export const DEFAULT_SETTINGS = {
  routing: { editHints: true, dedupeHints: true },
  checkpoint: { enabled: true },
  efficiency: {
    lookup: { enabled: true, reuse: true },
    batching: { enabled: true, maxConcurrency: 3 },
    attempts: { enabled: true, maxAttempts: 3 },
    advice: { enabled: true, at: 'meaningful-boundaries', nextStep: true },
    review: { mode: 'recommend' },
    context: { elevatedAt: 0.60, compactAt: 0.75, restartAt: 0.85, action: 'recommend' },
    delegation: { mode: 'recommend', maxWorkers: 3 }
  }
};
const object = value => value !== null && typeof value === 'object' && !Array.isArray(value) && Object.getPrototypeOf(value) === Object.prototype;
const enums = { mode: ['manual', 'recommend', 'execute-when-authorized'], action: ['manual', 'recommend', 'execute-when-authorized'], at: ['meaningful-boundaries'] };
function validate(value, template, label) {
  if (!object(value)) throw new Error(`${label} must be an object`);
  for (const [key, item] of Object.entries(value)) {
    if (!Object.hasOwn(template, key)) throw new Error(`unknown ${label} field: ${key}`);
    const expected = template[key], field = `${label}.${key}`;
    if (object(expected)) validate(item, expected, field);
    else if (typeof expected === 'boolean') {
      if (typeof item !== 'boolean') throw new Error(`${field} must be boolean`);
    } else if (typeof expected === 'string') {
      if (!enums[key].includes(item)) throw new Error(`invalid ${field}`);
    } else if (key.endsWith('At')) {
      if (!Number.isFinite(item) || item <= 0 || item > 1) throw new Error(`${field} must be greater than zero and at most one`);
    } else if (!Number.isSafeInteger(item) || item < (key === 'maxWorkers' ? 0 : 1) || item > (key === 'maxWorkers' ? Number.MAX_SAFE_INTEGER : key === 'maxAttempts' ? 5 : 64)) {
      throw new Error(`invalid ${field}`);
    }
  }
}
export function validateEfficiency(value) { validate(value, DEFAULT_SETTINGS.efficiency, 'efficiency'); }
export function validatePreferences(value) { validate(value, DEFAULT_SETTINGS, 'preferences'); }
function merge(target, source, origin, origins, prefix = '') {
  for (const [key, value] of Object.entries(source)) {
    const field = prefix ? `${prefix}.${key}` : key;
    if (object(value)) {
      if (!Object.hasOwn(target, key) || !object(target[key])) Object.defineProperty(target, key, { value: {}, enumerable: true, writable: true, configurable: true });
      merge(target[key], value, origin, origins, field);
    } else {
      Object.defineProperty(target, key, { value: Array.isArray(value) ? structuredClone(value) : value, enumerable: true, writable: true, configurable: true });
      origins[field] = origin;
    }
  }
}
export function effectiveSettings({ project, preferences = {}, overrides = {} }) {
  validatePreferences(preferences); validatePreferences(overrides);
  const settings = {}, origins = {};
  merge(settings, DEFAULT_SETTINGS, 'bundled', origins);
  if (project.execution?.profiles === undefined) {
    merge(settings, { execution: { profiles: DEFAULT_MODEL_PROFILES, roles: DEFAULT_MODEL_ROLES } }, 'bundled', origins);
  }
  merge(settings, preferences, 'user', origins);
  merge(settings, project, 'project', origins);
  merge(settings, overrides, 'task', origins);
  const { elevatedAt, compactAt, restartAt } = settings.efficiency.context;
  if (!(elevatedAt < compactAt && compactAt < restartAt)) throw new Error('context thresholds must satisfy elevatedAt < compactAt < restartAt');
  return { settings, origins, runtimeApplied: ['routing', 'checkpoint', 'efficiency.lookup', 'efficiency.attempts.maxAttempts', 'efficiency.context.thresholds', 'efficiency.context.action', 'efficiency.review.mode', 'efficiency.delegation.maxWorkers', 'efficiency.delegation.mode'], prerequisites: ['Automatic semantic acceptance and continuation require agent-supplied task evidence.', 'Native dispatch, worker-tree lifecycle observations and model switching need verified host integration; supplied facts and decision receipts do not establish execution.', 'Independent tool batching remains a workflow instruction; preferences never dispatch actions.'], authorization: 'Preferences do not grant permissions, host support, hook trust or execution authorization.' };
}
