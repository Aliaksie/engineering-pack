import { validateProject } from '../contracts/project.mjs';
// Capacity is a fresh host observation, independent of preferences and action approval.
export function workerBudget(project, facts = {}, { session, now } = {}) {
  validateProject(project);
  const allowed = ['capacity', 'occupiedSlots', 'taskWorkers', 'allowed', 'sessionId', 'observedAt', 'source'];
  if (!facts || typeof facts !== 'object' || Array.isArray(facts) || Object.keys(facts).some(key => !allowed.includes(key))) throw new Error('invalid worker capacity facts');
  for (const field of ['capacity', 'occupiedSlots', 'taskWorkers']) if (facts[field] !== undefined && (!Number.isSafeInteger(facts[field]) || facts[field] < (field === 'taskWorkers' ? 0 : 1))) throw new Error(`invalid worker ${field}`);
  if (facts.allowed !== undefined && typeof facts.allowed !== 'boolean') throw new Error('worker allowed must be boolean');
  const requested = project.efficiency?.delegation?.maxWorkers ?? 3;
  const base = { mode: project.efficiency?.delegation?.mode ?? 'recommend', requestedLimit: requested, effectiveLimit: null, available: 0, occupiedTaskWorkers: facts.taskWorkers ?? null,
    scope: 'One task-wide tree, including idle/reusable and nested workers that still occupy host slots.',
    evidence: 'Caller-supplied host observations; this calculation neither discovers capacity nor grants action authorization.' };
  if (requested === 0) return { ...base, effectiveLimit: 0, reason: 'disabled' };
  if (facts.allowed === false) return { ...base, effectiveLimit: 0, reason: 'session-prohibited' };
  const at = Date.parse(facts.observedAt);
  const fresh = session && facts.sessionId === session && typeof facts.source === 'string' && facts.source.length > 0 && facts.source.length <= 512
    && Number.isFinite(now) && Number.isFinite(at) && at <= now && now - at <= 30000;
  if (!fresh || facts.allowed !== true || [facts.capacity, facts.occupiedSlots, facts.taskWorkers].some(value => value === undefined)) return { ...base, reason: 'fresh-host-capacity-or-session-policy-unavailable' };
  if (facts.taskWorkers > facts.occupiedSlots - 1) throw new Error('worker slot counts exclude the main agent and must fit occupied slots');
  const otherOccupied = facts.occupiedSlots - 1 - facts.taskWorkers;
  const effectiveLimit = Math.min(requested, Math.max(0, facts.capacity - 1 - otherOccupied));
  return { ...base, effectiveLimit, available: Math.max(0, effectiveLimit - facts.taskWorkers),
    reason: facts.taskWorkers > effectiveLimit ? 'limit-reduced-no-new-assignments' : effectiveLimit < requested ? 'host-capacity-limited' : 'observed',
    observedAt: facts.observedAt, source: facts.source, cancelActiveWorkers: false };
}
