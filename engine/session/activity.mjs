import { digest } from '../cache/identity.mjs';
import { validateFeatures } from './features.mjs';
const identity = value => typeof value === 'string' && value.length > 0 && value.length <= 512 && !/[\x00-\x1f\x7f]/.test(value);
export function addActivity(state, values) {
  for (const [key, value] of Object.entries(values)) {
    const next = (state.activity[key] ?? 0) + value;
    if (!Number.isSafeInteger(next) || next < 0) throw new Error('session counter overflow');
    state.activity[key] = next;
  }
}

export function attemptReceipt(attempt, repository, id, at) {
  const fields = ['operation', 'target', 'approach', 'evidence', 'outcome', 'profile', 'maxAttempts', 'escalationReason'];
  if (!attempt || typeof attempt !== 'object' || Array.isArray(attempt) || Object.keys(attempt).some(key => !fields.includes(key))
    || !['operation', 'target', 'approach', 'evidence'].every(key => identity(attempt[key]))
    || !['passed', 'quality-failure', 'permission-denied', 'tool-unavailable', 'blocked'].includes(attempt.outcome)
    || (attempt.escalationReason !== undefined && (!identity(attempt.escalationReason) || !identity(attempt.profile)))
    || (attempt.profile !== undefined && !identity(attempt.profile))
    || (attempt.maxAttempts !== undefined && (!Number.isSafeInteger(attempt.maxAttempts) || attempt.maxAttempts < 1 || attempt.maxAttempts > 5))) throw new Error('invalid meaningful attempt receipt');
  const comparable = repository.gitAvailable && repository.fingerprints.complete;
  const source = comparable ? digest([repository.head, repository.fingerprints.index, repository.fingerprints.workingTree]) : null;
  const signature = source ? digest([attempt.operation, attempt.target, attempt.approach, source]) : null;
  return { ...attempt, id, source, signature, revision: repository.head, at };
}
export function unresolvedAttempts(records) {
  const lastPass = new Map();
  const key = record => JSON.stringify([record.operation, record.target]);
  records.forEach((record, index) => { if (record.outcome === 'passed') lastPass.set(key(record), index); });
  return records.filter((record, index) => record.outcome !== 'passed' && index > (lastPass.get(key(record)) ?? -1));
}
export function attemptStatus(records, operation) {
  const current = records.filter(record => record.operation === operation);
  const active = unresolvedAttempts(current);
  const blocker = active.findLast(record => ['permission-denied', 'tool-unavailable', 'blocked'].includes(record.outcome));
  const selected = blocker ?? active.at(-1) ?? current.at(-1);
  const target = current.filter(record => record.target === selected?.target);
  const lastPass = target.findLastIndex(record => record.outcome === 'passed');
  const last = target.at(-1);
  const series = target.slice(lastPass + 1);
  const failures = series.filter(record => !last?.profile || record.profile === last.profile);
  const equivalent = last?.signature ? failures.filter(record => record.signature === last.signature).length : 0;
  return { attempts: failures.length, maxAttempts: last?.maxAttempts ?? 3,
    noProgressAttempts: equivalent, retries: Math.max(0, failures.length - 1),
    blocked: series.some(record => ['permission-denied', 'tool-unavailable', 'blocked'].includes(record.outcome)),
    evidence: failures.map(record => record.evidence), records: series.length ? series : target.slice(-1) };
}

export function activityUpdate(state, values) { addActivity(state, values); return state; }
export function appendAttempt(state, receipt) {
  const records = [...state.attempts, receipt];
  // Successful operations close their series. Only closed history may be evicted;
  // unrelated work must never erase an unresolved blocker or retry budget.
  const unresolved = new Set(unresolvedAttempts(records));
  const active = records.map(record => unresolved.has(record));
  const activeCount = active.filter(Boolean).length;
  if (activeCount > 24) throw new Error('session attempt capacity reached: preserve evidence and resolve an active operation before recording more attempts');
  addActivity(state, { operationsAttempted: 1, ...(receipt.escalationReason ? { modelEscalations: 1 } : {}) });
  let discard = Math.max(0, records.length - 24);
  state.attempts = records.filter((record, index) => {
    if (!active[index] && discard > 0) { discard--; return false; }
    return true;
  });
  return state;
}

export function validateActivityState(state) {
  validateFeatures(state.featureOutcomes);
  if (state.recommendations !== undefined) {
    if (!state.recommendations || typeof state.recommendations !== 'object' || Array.isArray(state.recommendations)
      || Object.keys(state.recommendations).length > 16) throw new Error('invalid recommendation state');
    for (const [id, offer] of Object.entries(state.recommendations)) {
      if (!/^[a-f0-9]{64}$/.test(id) || !offer || offer.id !== id) throw new Error('invalid recommendation identity');
      const receipt = offer.executionReceipt;
      if (receipt !== undefined && (!receipt || Object.keys(receipt).some(key => !['key', 'state', 'source', 'checkpoint', 'generation', 'evidence', 'acceptanceEvidence'].includes(key))
        || receipt.key !== digest([id, 'execute']) || receipt.source !== offer.source
        || !['dispatching', 'started', 'completed', 'not-started', 'uncertain'].includes(receipt.state)
        || (receipt.checkpoint !== null && !/^[a-f0-9]{64}$/.test(receipt.checkpoint ?? ''))
        || !Number.isSafeInteger(receipt.generation) || receipt.generation < 0
        || [receipt.evidence, receipt.acceptanceEvidence].some(value => value !== null && !identity(value)))) throw new Error('invalid native action receipt');
    }
  }
  if (state.formatVersion !== 1 || !state.activity || typeof state.activity !== 'object' || Array.isArray(state.activity)
    || !state.lookups || typeof state.lookups !== 'object' || Array.isArray(state.lookups)
    || !Array.isArray(state.attempts) || state.attempts.length > 24
    || Object.keys(state.lookups).length > 32
    || (state.lastDiscovery !== undefined && (!state.lastDiscovery || !/^[a-f0-9]{64}$/.test(state.lastDiscovery.key) || !Number.isSafeInteger(state.lastDiscovery.repetitions) || state.lastDiscovery.repetitions < 0))
    || Object.values(state.activity).some(value => !Number.isSafeInteger(value) || value < 0)) throw new Error('invalid session state');
  return state;
}

export function activityIdentity(session, task) {
  if (!identity(session) || !identity(task)) throw new Error('session activity requires bounded session and task identities');
  return true;
}
