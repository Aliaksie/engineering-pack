// Exact host mappings are declarations, never a model switch or permission grant.
export function delegation(project, role, host, profile, fallback = { mode: 'main', role }) {
  if (project.efficiency?.delegation?.maxWorkers === 0) return fallback;
  const rule = project.execution?.roles?.[role];
  const mapping = project.execution?.hosts?.[host];
  if (rule?.mode !== 'delegate' || mapping?.delegation !== true) return fallback;
  const model = profile && mapping.models && Object.hasOwn(mapping.models, profile) ? mapping.models[profile] : undefined;
  if (profile && (typeof model !== 'string' || !model)) return fallback;
  return { mode: 'delegate', role, ...(model ? { model, profile } : {}) };
}
