import { digest } from '../cache/identity.mjs';
import { workerBudget } from '../routing/worker-budget.mjs';

const text = value => typeof value === 'string' && value.length > 0 && value.length <= 512 && !/[\x00-\x1f\x7f]/.test(value);
const identifier = value => typeof value === 'string' && /^[A-Za-z0-9][A-Za-z0-9._:-]{0,95}$/.test(value);
const hash = value => typeof value === 'string' && /^[a-f0-9]{64}$/.test(value);
const states = ['reserved', 'dispatching', 'cancelling', 'observed', 'uncertain'];
const statuses = ['running', 'idle', 'completed', 'failed', 'cancelled', 'released', 'not-started', 'unknown'];
function keys(value, allowed, label) {
  if (!value || typeof value !== 'object' || Array.isArray(value) || Object.keys(value).some(key => !allowed.includes(key))) throw new Error(`invalid ${label}`);
}
export function workerInput(input) {
  keys(input, ['id', 'offer', 'scope', 'parent', 'reuseWorker', 'brief'], 'worker assignment');
  if (!identifier(input.id) || !hash(input.offer) || !text(input.scope)
    || (input.parent !== undefined && !identifier(input.parent)) || (input.reuseWorker !== undefined && !text(input.reuseWorker))) throw new Error('invalid worker assignment identity/scope');
  keys(input.brief, ['goal', 'sources', 'acceptance', 'restrictions', 'returnShape'], 'worker brief');
  if (!text(input.brief.goal) || !text(input.brief.returnShape)) throw new Error('worker goal and return shape are required');
  for (const field of ['sources', 'acceptance', 'restrictions']) if (!Array.isArray(input.brief[field]) || input.brief[field].length > 16
    || (field === 'acceptance' && !input.brief[field].length) || input.brief[field].some(value => !text(value))) throw new Error(`invalid worker brief ${field}`);
  return input;
}
export function workerState(state, tree) {
  if (!hash(tree)) throw new Error('invalid worker tree identity');
  if (state == null) return { formatVersion: 1, tree, requests: [] };
  keys(state, ['formatVersion', 'tree', 'requests'], 'worker state');
  if (state.formatVersion !== 1 || !hash(tree) || state.tree !== tree || !Array.isArray(state.requests) || state.requests.length > 64) throw new Error('invalid worker tree state');
  const seen = new Set();
  for (const request of state.requests) {
    keys(request, ['id', 'key', 'inputHash', 'offer', 'scope', 'source', 'policy', 'host', 'model', 'role', 'profile', 'parent', 'reuseWorker',
      'state', 'status', 'workerId', 'occupied', 'generation', 'evidence', 'modelMatches', 'cancelKey', 'revision', 'at'], 'worker request state');
    if (!identifier(request.id) || seen.has(request.id) || !hash(request.key) || request.key !== digest([tree, request.id]) || !hash(request.inputHash)
      || !hash(request.offer) || !hash(request.source) || !hash(request.policy)
      || !['scope', 'host', 'model', 'role', 'profile', 'at'].every(field => text(request[field]))
      || !Number.isFinite(Date.parse(request.at)) || !states.includes(request.state) || !statuses.includes(request.status)
      || typeof request.occupied !== 'boolean' || typeof request.modelMatches !== 'boolean'
      || !Number.isSafeInteger(request.generation) || request.generation < 0
      || (request.workerId !== null && !text(request.workerId)) || (request.evidence !== null && !text(request.evidence))
      || (request.cancelKey !== null && !hash(request.cancelKey))
      || (request.revision !== null && (typeof request.revision !== 'string' || !/^[a-f0-9]{40,64}$/.test(request.revision)))
      || (request.parent !== null && !identifier(request.parent)) || (request.reuseWorker !== null && !text(request.reuseWorker))) throw new Error('invalid saved worker request');
    seen.add(request.id);
  }
  return state;
}
export function workerSnapshot(snapshot, session, now) {
  keys(snapshot, ['host', 'capacity', 'occupiedSlots', 'taskWorkers', 'allowed', 'sessionId', 'observedAt', 'source', 'workers', 'requests'], 'host worker snapshot');
  if (!text(snapshot.host) || !Array.isArray(snapshot.workers) || snapshot.workers.length > 256
    || !Array.isArray(snapshot.requests) || snapshot.requests.length > 128) throw new Error('bounded worker inventory is required');
  const at = Date.parse(snapshot.observedAt);
  if (snapshot.sessionId !== session || !text(snapshot.source) || !Number.isFinite(at) || !Number.isFinite(now) || at > now || now - at > 30000
    || typeof snapshot.allowed !== 'boolean' || !Number.isSafeInteger(snapshot.capacity) || snapshot.capacity < 1
    || !Number.isSafeInteger(snapshot.occupiedSlots) || snapshot.occupiedSlots < 1 || snapshot.occupiedSlots > snapshot.capacity
    || !Number.isSafeInteger(snapshot.taskWorkers) || snapshot.taskWorkers < 0) throw new Error('fresh matching host capacity is required');
  const ids = new Set(), operations = new Set();
  for (const worker of snapshot.workers) {
    keys(worker, ['id', 'status', 'occupiesSlot', 'model', 'evidence'], 'host worker');
    observation(worker);
    if (!text(worker.id) || ids.has(worker.id)) throw new Error('invalid or duplicate host worker id');
    ids.add(worker.id);
  }
  for (const operation of snapshot.requests) {
    keys(operation, ['key', 'status', 'workerId', 'evidence', 'fenced', 'settled'], 'host dispatch lookup');
    if (!hash(operation.key) || operations.has(operation.key) || !['not-started', 'started'].includes(operation.status) || !text(operation.evidence)
      || (operation.status === 'started' && (!text(operation.workerId) || !ids.has(operation.workerId) || operation.settled !== true))
      || (operation.status === 'not-started' && (operation.workerId != null || operation.fenced !== true))) throw new Error('a negative dispatch lookup must fence late execution');
    operations.add(operation.key);
  }
  const occupied = snapshot.workers.filter(worker => worker.occupiesSlot).length;
  if (snapshot.taskWorkers !== occupied || occupied > snapshot.occupiedSlots - 1) throw new Error('host task worker counts must match the complete tree inventory');
  return snapshot;
}
function observation(value) {
  if (!statuses.includes(value.status) || typeof value.occupiesSlot !== 'boolean' || !text(value.evidence)
    || (value.model !== null && !text(value.model)) || (['running', 'idle'].includes(value.status) && !value.occupiesSlot)
    || (['released', 'not-started'].includes(value.status) && value.occupiesSlot)
    || (value.status === 'unknown' && !value.occupiesSlot)) throw new Error('invalid native worker observation');
}
function slotKey(request) { return request.workerId ? `worker:${request.workerId}` : `request:${request.key}`; }
export function reconcileWorkers(state, snapshot, targets) {
  for (const request of state.requests) {
    if (request.host !== snapshot.host) continue;
    if (!targets.some(target => target.key === request.key && target.generation === request.generation)) continue;
    const lookup = snapshot.requests.find(value => value.key === request.key);
    const cancel = request.cancelKey ? snapshot.requests.find(value => value.key === request.cancelKey) : null;
    const pending = request.state !== 'observed';
    if (!request.workerId && lookup?.status === 'not-started') {
      Object.assign(request, { state: 'observed', status: 'not-started', occupied: false, evidence: lookup.evidence }); request.generation++;
      continue;
    }
    const workerId = request.workerId ?? (lookup?.status === 'started' ? lookup.workerId : null);
    const found = snapshot.workers.find(value => value.id === workerId);
    if (found) {
      // An idle/running worker does not settle an in-flight follow-up or cancellation.
      // Resolve an ambiguous operation only from its final receipt, a fence, or slot release.
      if (pending && found.occupiesSlot && !(request.cancelKey ? cancel : lookup)) continue;
      Object.assign(request, { workerId, state: 'observed', status: pending && !request.cancelKey && lookup?.status === 'not-started' ? 'not-started' : found.status, occupied: found.occupiesSlot,
        modelMatches: found.model === request.model, evidence: found.evidence }); request.generation++;
    }
    // Missing entries never prove dispatch failure, completion, or slot release.
  }
  return state;
}
export function workerAdmission({ state, input, checked, snapshot, project, session, now, at, revision }) {
  workerInput(input);
  const inputHash = digest(input), existing = state.requests.find(request => request.id === input.id);
  if (existing) {
    if (existing.inputHash !== inputHash) throw new Error('worker assignment id already has different content');
    return { state, request: existing, duplicate: true };
  }
  if (!checked?.ready || checked.offer?.action !== 'delegate' || checked.offer.scope !== input.scope
    || checked.offer.id !== input.offer || checked.offer.host !== snapshot.host) throw new Error('current scoped worker approval and host mapping are required');
  const offer = checked.offer;
  if (state.requests.some(request => request.occupied && request.host !== snapshot.host)) throw new Error('occupied workers on another host require that host to reconcile before switching transport');
  const held = new Set(state.requests.filter(request => request.occupied).map(slotKey));
  for (const worker of snapshot.workers.filter(worker => worker.occupiesSlot)) held.delete(`worker:${worker.id}`);
  const facts = Object.fromEntries(['capacity', 'occupiedSlots', 'taskWorkers', 'allowed', 'sessionId', 'observedAt', 'source'].map(field => [field, snapshot[field]]));
  facts.occupiedSlots += held.size; facts.taskWorkers += held.size;
  const budget = workerBudget(project, facts, { session, now });
  if (input.parent) {
    const parent = state.requests.find(request => request.id === input.parent);
    if (!parent || parent.status !== 'running' || !parent.occupied || !parent.modelMatches) throw new Error('nested dispatch requires a current observed parent in the same tree');
  }
  if (budget.effectiveLimit == null || budget.effectiveLimit < 1 || facts.taskWorkers > budget.effectiveLimit) throw new Error('worker ceiling prohibits new assignments');
  if (input.reuseWorker) {
    const worker = snapshot.workers.find(value => value.id === input.reuseWorker);
    const known = state.requests.findLast(value => value.workerId === input.reuseWorker);
    if (!known || !worker?.occupiesSlot || worker.status !== 'idle' || worker.model !== offer.model
      || state.requests.some(value => value.workerId === input.reuseWorker && ['reserved', 'dispatching', 'cancelling', 'uncertain'].includes(value.state))) throw new Error('reuse requires an observed idle mapped worker without pending assignments');
  } else if (budget.available < 1) throw new Error('worker capacity is reserved or occupied');
  const busy = new Set(state.requests.filter(value => value.offer === offer.id && value.occupied && (value.status === 'running' || value.state !== 'observed')).map(slotKey));
  if (busy.size >= (offer.proposedWorkers ?? 0)) throw new Error('approved concurrent assignment scope is full');
  if (state.requests.length >= 64) throw new Error('worker evidence capacity reached; no records were evicted');
  const request = { id: input.id, key: digest([state.tree, input.id]), inputHash, offer: offer.id, scope: input.scope,
    source: offer.source, policy: offer.policy, host: offer.host, model: offer.model, role: offer.role, profile: offer.profile, revision: revision ?? null,
    parent: input.parent ?? null, reuseWorker: input.reuseWorker ?? null, state: 'reserved', status: 'unknown',
    workerId: input.reuseWorker ?? null, occupied: true, generation: 0, evidence: null, modelMatches: false, cancelKey: null, at };
  state.requests.push(request);
  return { state, request, duplicate: false, budget };
}
export function workerTransition(state, id, result) {
  const request = state.requests.find(value => value.id === id);
  if (!request) throw new Error('unknown worker assignment');
  if (result.kind === 'cancelling') {
    if (!request.workerId || !request.occupied || request.state === 'cancelling' || (request.state === 'uncertain' && request.cancelKey)
      || state.requests.findLast(value => value.workerId === request.workerId).id !== id) throw new Error('cancellation requires the latest occupied worker assignment');
    request.cancelKey = digest([request.key, 'cancel', request.generation]);
    request.state = 'cancelling'; request.generation++; return { state, request };
  }
  if (result.kind === 'dispatching') {
    if (request.state !== 'reserved') throw new Error('worker dispatch was already attempted');
    request.state = 'dispatching'; request.generation++; return { state, request };
  }
  if (result.kind === 'uncertain') {
    request.state = 'uncertain'; request.status = 'unknown'; request.occupied = true; request.generation++;
    return { state, request };
  }
  keys(result, ['kind', 'workerId', 'status', 'occupiesSlot', 'model', 'evidence'], 'native worker result');
  if (result.kind !== 'observed' || !text(result.workerId)) throw new Error('a native worker result requires its actual id');
  observation(result);
  if (request.workerId && result.workerId !== request.workerId) throw new Error('native worker identity changed');
  if (!request.reuseWorker && state.requests.some(value => value.id !== id && value.occupied && value.workerId === result.workerId)) throw new Error('native worker id is already assigned');
  Object.assign(request, { workerId: result.workerId, state: 'observed', status: result.status, occupied: result.occupiesSlot,
    modelMatches: result.model === request.model, evidence: result.evidence }); request.generation++;
  return { state, request };
}
