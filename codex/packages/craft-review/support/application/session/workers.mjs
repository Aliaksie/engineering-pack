import { decide, digest } from '../engine-client.mjs';
import { readProject } from '../project/configuration.mjs';
import { taskRepository, activeTask, bindingFile, readSessionState, updateSessionState } from './activity.mjs';
import { stateFile, realRoot, readState, writeState, lockState, releaseState, clock } from '../../platform/index.mjs';
import { withTelemetry, observe } from '../telemetry/observe.mjs';

const MAX_BYTES = 262144;
const clone = value => JSON.parse(JSON.stringify(value));
const executionKnown = request => request.state === 'observed' && request.workerId !== null && !['unknown', 'not-started'].includes(request.status);
function storage(root, session, task) {
  decide('activity-identity', { session, task });
  const tree = digest([realRoot(root), session, task]);
  const file = stateFile(root, `workers-${tree.slice(0, 24)}.json`);
  const read = () => decide('worker-state', { state: readState(file, MAX_BYTES), tree });
  const update = callback => {
    stateFile(root, `workers-${tree.slice(0, 24)}.json`, true);
    const ticket = lockState(file);
    if (!ticket) throw new Error('worker tree writer is busy');
    try {
      const state = read(), result = callback(state);
      writeState(file, state, MAX_BYTES);
      return result;
    } finally { releaseState(ticket); }
  };
  return { tree, read, update };
}
export function workerStatus(root, session, task) {
  const store = storage(root, session, task), state = store.read();
  return { tree: store.tree, requests: state.requests, transport: 'Host callbacks are required; no native transport is installed by configuration.',
    limits: { maxRequests: 64, maxBytes: MAX_BYTES }, acceptance: 'Worker lifecycle evidence never closes main-task acceptance.' };
}

// The caller supplies trusted host functions. Project JSON cannot load executable adapters,
// grant authority, invent a model mapping, or select an ambient CLI as a native dispatcher.
export function workerController(root, { session, task, preferencesFile } = {}, host = {}) {
  root = realRoot(root);
  const store = storage(root, session, task), context = { tree: store.tree, root, session, task };
  const supported = ['observe', 'authorize', 'dispatch'].every(name => typeof host[name] === 'function');
  const unsupported = () => ({ supported: false, dispatchAttempted: false, executionObserved: false,
    reason: 'A trusted host adapter with observe/authorize/dispatch callbacks is required; no native worker command is inferred.' });
  const targets = state => state.requests.map(request => ({ key: request.key, cancelKey: request.cancelKey, generation: request.generation, workerId: request.workerId }));
  const snapshot = async state => decide('worker-snapshot', { snapshot: await host.observe(clone({ ...context, requests: targets(state) })), session, now: clock().ms });
  const reconcile = async () => {
    if (typeof host.observe !== 'function') return unsupported();
    const before = store.read(), observed = await snapshot(before);
    store.update(state => Object.assign(state, decide('worker-reconcile', { state, snapshot: observed, targets: targets(before) })));
    return { ...workerStatus(root, session, task), host: observed.host, observedAt: observed.observedAt };
  };
  const record = (id, result, expectedGeneration) => store.update(state => {
    const current = state.requests.find(request => request.id === id);
    if (expectedGeneration !== undefined && current?.generation !== expectedGeneration) return current;
    const next = decide('worker-transition', { state, id, result }); Object.assign(state, next.state); return next.request;
  });
  const observedResult = (id, result, expectedGeneration) => {
    try {
      const request = record(id, { ...result, kind: 'observed' }, expectedGeneration);
      try {
        updateSessionState(root, session, task, state => {
          const offer = state.recommendations?.[request.offer];
          if (offer) offer.execution = request.modelMatches ? 'host-observed' : 'host-observed-model-mismatch';
          if (executionKnown(request)) Object.assign(state, decide('feature-record', { state, source: request.source, at: clock().iso, provenance: 'runtime-observed',
            input: { feature: 'delegation', status: request.modelMatches ? 'used' : 'failed', evidence: request.evidence,
              reason: request.modelMatches ? 'Host adapter observed the configured worker; main acceptance remains separate.' : 'Host adapter reported a worker model mismatch; its occupied slot remains counted.' } }).state);
        });
      } catch { /* The durable reservation is the recovery owner, not the advice receipt. */ }
      return { supported: true, dispatchAttempted: true, executionObserved: executionKnown(request), modelMatches: request.modelMatches, request };
    } catch {
      try { record(id, { kind: 'uncertain' }, expectedGeneration); } catch { /* Existing dispatching reservation stays occupied. */ }
      return { supported: true, dispatchAttempted: true, executionObserved: false, uncertain: true,
        recoveryKey: digest([store.tree, id]), reason: 'Native response or persistence could not be verified; keep its reservation and reconcile before further dispatch.' };
    }
  };
  const dispatch = async input => {
    decide('worker-input', { input });
    if (!supported) return unsupported();
    const before = store.read(), existing = before.requests.find(request => request.id === input.id);
    if (existing) {
      if (existing.inputHash !== digest(input)) throw new Error('worker assignment id already has different content');
      return { supported: true, duplicate: true, dispatchAttempted: false, executionObserved: executionKnown(existing), request: existing };
    }
    const authority = await host.authorize(clone({ ...context, action: 'dispatch', input }));
    if (authority?.allowed !== true || typeof authority.evidence !== 'string' || !authority.evidence || authority.evidence.length > 512) return { supported: true, dispatchAttempted: false, executionObserved: false, reason: 'The host did not verify current scoped authority.' };
    const observed = await snapshot(before);
    const binding = lockState(bindingFile(root, session));
    if (!binding) throw new Error('active task writer is busy');
    let admitted;
    try {
      if (activeTask(root, session) !== task) throw new Error('worker dispatch requires the active coordinator task binding');
      const project = readProject(root, { preferencesFile }), repository = taskRepository(root, session, task);
      admitted = store.update(state => {
        Object.assign(state, decide('worker-reconcile', { state, snapshot: observed, targets: targets(before) }));
        const facts = { observations: { host: { name: observed.host } }, allowed: observed.allowed, supported: true,
          independentTasks: 2, reuseAvailable: Boolean(input.reuseWorker && observed.workers.some(worker => worker.id === input.reuseWorker && worker.status === 'idle' && worker.occupiesSlot)),
          workers: Object.fromEntries(['capacity', 'occupiedSlots', 'taskWorkers', 'allowed', 'sessionId', 'observedAt', 'source'].map(field => [field, observed[field]])) };
        const checked = decide('action-check', { state: readSessionState(root, session, task), input: { id: input.offer }, project, repository, facts, session, now: clock().ms, status: {} });
        const result = decide('worker-admit', { state, input, checked, snapshot: observed, project, session, now: clock().ms, at: clock().iso, revision: repository.head });
        Object.assign(state, result.state);
        if (!result.duplicate) Object.assign(state, decide('worker-transition', { state, id: input.id, result: { kind: 'dispatching' } }).state);
        return { ...result, request: state.requests.find(request => request.id === input.id) };
      });
    } finally { releaseState(binding); }
    if (admitted.duplicate) return { supported: true, duplicate: true, dispatchAttempted: false, executionObserved: executionKnown(admitted.request), request: admitted.request };
    try {
      const result = await host.dispatch(clone({ ...context, key: admitted.request.key, model: admitted.request.model,
        role: admitted.request.role, profile: admitted.request.profile, source: admitted.request.source, revision: admitted.request.revision,
        parent: input.parent ?? null, reuseWorker: input.reuseWorker ?? null, brief: input.brief, scope: input.scope, authorizationEvidence: authority.evidence }));
      return observedResult(input.id, result, admitted.request.generation);
    } catch {
      try { record(input.id, { kind: 'uncertain' }, admitted.request.generation); } catch { /* Persisted dispatching request continues holding capacity. */ }
      return { supported: true, dispatchAttempted: true, executionObserved: false, uncertain: true, recoveryKey: admitted.request.key,
        reason: 'Native dispatch outcome is unknown. No retry or capacity release was inferred; reconcile the operation key.' };
    }
  };
  const cancel = async id => {
    if (typeof host.cancel !== 'function' || typeof host.authorize !== 'function' || typeof host.observe !== 'function') return unsupported();
    const request = store.read().requests.find(value => value.id === id);
    if (!request?.workerId || !request.occupied) throw new Error('cancellation requires an observed occupied native worker');
    if ((await snapshot(store.read())).host !== request.host) throw new Error('cancellation host identity does not match the occupied worker');
    const authority = await host.authorize(clone({ ...context, action: 'cancel', request }));
    if (authority?.allowed !== true) return { supported: true, cancelled: false, reason: 'The host did not verify cancellation authority.' };
    const intent = record(id, { kind: 'cancelling' });
    try { return { ...observedResult(id, await host.cancel(clone({ ...context, key: intent.cancelKey, workerId: request.workerId })), intent.generation), dispatchAttempted: false, cancellationRequested: true }; }
    catch {
      try { record(id, { kind: 'uncertain' }, intent.generation); } catch { /* The cancellation intent still holds its occupied slot. */ }
      return { supported: true, cancellationRequested: true, uncertain: true, reason: 'Cancellation outcome is unknown; its occupied slot was not released.' };
    }
  };
  return { status: () => workerStatus(root, session, task), reconcile,
    dispatch: input => withTelemetry(root, { session }, () => observe('worker.dispatch', () => dispatch(input), result => ({ available: result.supported, complete: result.executionObserved, ...(result.uncertain ? { reason: 'incomplete' } : {}) }))),
    cancel, supported };
}
