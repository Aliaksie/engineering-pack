import { nativeWorkers } from '../platform/index.mjs';
import { forwardTelemetry } from '../application/telemetry/observe.mjs';

const clone = value => JSON.parse(JSON.stringify(value));
// Host callbacks carry actual observations and authority. Native phases own validation,
// reservations and all state transitions; no lock spans an asynchronous callback here.
export function workerController(root, { session, task, preferencesFile } = {}, host = {}) {
  const call = (phase, input = {}) => {
    const response = nativeWorkers({ root, session, task, preferencesFile, phase, ...input });
    forwardTelemetry(response.export);
    if (response.error) throw new Error(response.error);
    return response.value;
  };
  const { context } = call('open');
  const supported = ['observe', 'authorize', 'dispatch'].every(name => typeof host[name] === 'function');
  const unsupported = () => ({ supported: false, dispatchAttempted: false, executionObserved: false,
    reason: 'A trusted host adapter with observe/authorize/dispatch callbacks is required; no native worker command is inferred.' });
  const snapshot = async targets => host.observe(clone({ ...context, requests: targets }));
  const reconcile = async () => {
    if (typeof host.observe !== 'function') return unsupported();
    const { targets } = call('inventory');
    return call('reconcile', { targets, snapshot: await snapshot(targets) });
  };
  const record = (id, result, generation) => call('record', { id, result, ...(generation === undefined ? {} : { generation }) });
  const uncertain = (id, generation) => { try { record(id, { kind: 'uncertain' }, generation); } catch { /* The durable reservation remains the recovery owner. */ } };
  const observedResult = (id, result, generation, recoveryKey) => {
    try {
      const recorded = record(id, { ...result, kind: 'observed' }, generation);
      return { supported: true, dispatchAttempted: true, ...recorded };
    } catch {
      uncertain(id, generation);
      return { supported: true, dispatchAttempted: true, executionObserved: false, uncertain: true, recoveryKey,
        reason: 'Native response or persistence could not be verified; keep its reservation and reconcile before further dispatch.' };
    }
  };
  const dispatch = async input => {
    const before = call('prepare', { input, inspect: supported });
    input = clone(input);
    if (!supported) return unsupported();
    if (before.existing) return before.existing;
    const authority = await host.authorize(clone({ ...context, action: 'dispatch', input }));
    if (authority?.allowed !== true || typeof authority.evidence !== 'string' || !authority.evidence || authority.evidence.length > 512) return { supported: true, dispatchAttempted: false, executionObserved: false, reason: 'The host did not verify current scoped authority.' };
    const admitted = call('admit', { input, targets: before.targets, snapshot: await snapshot(before.targets) });
    if (admitted.duplicate) return call('prepare', { input }).existing;
    const request = admitted.request;
    try {
      const result = await host.dispatch(clone({ ...context, key: request.key, model: request.model,
        role: request.role, profile: request.profile, source: request.source, revision: request.revision,
        taskContract: request.taskContract ?? null, parent: input.parent ?? null, reuseWorker: input.reuseWorker ?? null,
        brief: input.brief, scope: input.scope, authorizationEvidence: authority.evidence }));
      return observedResult(input.id, result, request.generation, request.key);
    } catch {
      uncertain(input.id, request.generation);
      return { supported: true, dispatchAttempted: true, executionObserved: false, uncertain: true, recoveryKey: request.key,
        reason: 'Native dispatch outcome is unknown. No retry or capacity release was inferred; reconcile the operation key.' };
    }
  };
  const cancel = async id => {
    if (!['cancel', 'authorize', 'observe'].every(name => typeof host[name] === 'function')) return unsupported();
    const before = call('inventory', { id });
    const request = call('cancel-check', { request: before.request, snapshot: await snapshot(before.targets) });
    const authority = await host.authorize(clone({ ...context, action: 'cancel', request }));
    if (authority?.allowed !== true) return { supported: true, cancelled: false, reason: 'The host did not verify cancellation authority.' };
    const { request: intent } = record(id, { kind: 'cancelling' });
    try { return { ...observedResult(id, await host.cancel(clone({ ...context, key: intent.cancelKey, workerId: request.workerId })), intent.generation, request.key), dispatchAttempted: false, cancellationRequested: true }; }
    catch {
      uncertain(id, intent.generation);
      return { supported: true, cancellationRequested: true, uncertain: true, reason: 'Cancellation outcome is unknown; its occupied slot was not released.' };
    }
  };
  return { status: () => call('status'), reconcile, dispatch, cancel, supported };
}
