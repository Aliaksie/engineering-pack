// Logical classes are declarations, not measured rankings or vendor model names.
export const DEFAULT_MODEL_PROFILES = {
  economical: { class: 'economical', capabilities: [], escalateTo: 'standard' },
  standard: { class: 'standard', capabilities: [], escalateTo: 'strong' },
  strong: { class: 'strong', capabilities: [] }
};
export const DEFAULT_MODEL_ROLES = Object.fromEntries(['locator', 'researcher', 'implementer', 'reviewer', 'planner', 'verifier'].map(role => [role,
  { mode: role === 'locator' ? 'tool' : 'main', profile: role === 'locator' ? 'economical' : role === 'reviewer' ? 'strong' : 'standard' }]));
