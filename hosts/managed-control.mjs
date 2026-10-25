import { workerController } from './workers.mjs';
import { actionController } from './actions.mjs';
import { nativeJournal, withTaskAdmission, assertNativeSource } from './native-control.mjs';
import { clock, realRoot, sameRoot } from '../platform/index.mjs';
export { assertNativeSource } from './native-control.mjs';

export const nativeText = value => typeof value === 'string' && value.length > 0 && value.length <= 512 && !/[\x00-\x1f\x7f]/.test(value);
export const nativeCopy = value => JSON.parse(JSON.stringify(value));
// A driver supplies native transport facts. Rust owns the durable journal and
// admission fence; this adapter sequences callbacks using opaque native tickets.
export function managedControls({ host, root, session, task, coordinatorId, capacity, driver,
  authorize, observations, verifyResume, preferencesFile, bundle } = {}) {
  root = realRoot(root);
  if (!nativeText(host) || !nativeText(coordinatorId) || !Number.isSafeInteger(capacity) || capacity < 1 || capacity > 64
    || typeof authorize !== 'function' || !driver || !['snapshot', 'start', 'cancel', 'act', 'lookup', 'capabilities', 'output', 'close'].every(key => typeof driver[key] === 'function')) throw new Error('explicit native driver, identity, capacity and authority verifier required');
  const owner = nativeJournal({ root, session, task, host, coordinator: coordinatorId, mode: 'managed' });
  const { context, journal } = owner;
  let closed = false, broken = false;
  const usable = () => { if (closed || broken) throw new Error('native owner unavailable; retained intent requires reconciliation'); };
  const persist = withdrawn => { usable(); try { owner.persist(withdrawn); } catch (error) { broken = true; throw error; } };
  const binding = input => {
    usable();
    if (!sameRoot(input.root, root) || input.session !== session || input.task !== task || (input.tree !== undefined && input.tree !== context.tree)) throw new Error('native coordinator binding changed');
  };
  const authority = async input => { binding(input); return authorize(nativeCopy(input)); };
  const guard = async input => {
    const decision = await authority(input);
    if (decision?.allowed !== true || !nativeText(decision.evidence)) throw new Error('fresh scoped native authority is required');
    if (input.source !== undefined) assertNativeSource(root, input.source, session, task);
  };
  const begin = (input, kind, nativeId = null) => {
    binding(input);
    return owner.begin(input, kind, nativeId);
  };
  const remember = op => id => { if (!nativeText(id)) throw new Error('invalid native identity'); op.nativeId = id; persist(); };
  const acknowledged = (op, value) => { if (value?.workerId) remember(op)(value.workerId); op.phase = 'acknowledged'; persist(); return value; };
  const snapshot = async input => {
    binding(input);
    const ids = [...new Set(journal.operations.filter(op => op.kind === 'worker' && op.nativeId).map(op => op.nativeId))];
    const actual = await driver.snapshot(ids);
    if (!Array.isArray(actual?.workers) || actual.workers.length > 64 || !Number.isSafeInteger(actual.otherOccupied ?? 0) || (actual.otherOccupied ?? 0) < 0) throw new Error('bounded native inventory unavailable');
    const workers = ids.map(id => {
      const released = journal.operations.some(op => op.kind === 'release' && op.nativeId === id && op.released);
      return released ? { id, status: 'released', occupiesSlot: false, model: null, evidence: `${host}:released:${id}` }
        : actual.workers.find(worker => worker.id === id) ?? { id, status: 'unknown', occupiesSlot: true, model: null, evidence: `${host}:unresolved:${id}` };
    });
    const requests = journal.operations.filter(op => ['worker', 'cancel'].includes(op.kind) && op.phase === 'acknowledged'
      && workers.some(worker => worker.id === op.nativeId && worker.status !== 'unknown')
      && (op.kind !== 'cancel' || actual.settledCancellations?.includes(op.key)))
      .map(op => ({ key: op.key, status: 'started', settled: true, workerId: op.nativeId, evidence: `${host}:${op.key}` }));
    const occupied = workers.filter(worker => worker.occupiesSlot).length;
    return { host, capacity, occupiedSlots: 1 + occupied + (actual.otherOccupied ?? 0), taskWorkers: occupied, allowed: actual.allowed === true,
      sessionId: session, observedAt: clock().iso, source: `${host}:exclusive-native-lane`, workers, requests };
  };
  const workerHost = { authorize: authority, observe: snapshot,
    async dispatch(input) {
      await guard({ ...input, action: 'dispatch' });
      const view = await snapshot(input);
      if (view.allowed !== true || (!input.reuseWorker && view.occupiedSlots >= capacity)) throw new Error('native lane capacity or policy prohibits dispatch');
      return withTaskAdmission(input, async assertCurrent => {
        const op = begin(input, 'worker', input.reuseWorker ?? null);
        const result = await driver.start(nativeCopy(input), remember(op), assertCurrent);
        return acknowledged(op, result);
      });
    },
    async cancel(input) {
      await guard({ ...input, action: 'cancel' });
      return withTaskAdmission(input, async () => {
        const op = begin(input, 'cancel', input.workerId);
        // A cancellation acknowledgement does not settle the original prompt.
        acknowledged(op, await driver.cancel(nativeCopy(input)));
        const view = await snapshot(input), result = view.workers.find(worker => worker.id === input.workerId);
        if (!view.requests.some(request => request.key === input.key) || !result) throw new Error('native cancellation has not settled');
        const { id, ...value } = result; return { workerId: id, ...value };
      });
    }
  };
  const actionHost = { authorize: authority,
    async observe(input) {
      binding(input);
      if (typeof observations !== 'function') throw new Error('real action observations required');
      const value = await observations(nativeCopy(input));
      const support = driver.capabilities();
      return { ...value, facts: { ...value.facts, supported: value.facts?.supported === true && (input.action === 'compact' ? support.compaction : support.review),
        observations: { ...value.facts?.observations, host: { ...value.facts?.observations?.host, name: host,
          reliableCompaction: support.compaction === true && typeof verifyResume === 'function' } } } };
    },
    async execute(input) {
      await guard(input);
      const support = driver.capabilities();
      if (!(input.action === 'compact' ? support.compaction && typeof verifyResume === 'function' : input.action === 'review' && support.review)) throw new Error('native action unsupported');
      if (journal.operations.some(op => ['review', 'compact'].includes(op.kind) && op.phase !== 'completed')) throw new Error('unresolved coordinator action must be reconciled before another native action');
      return withTaskAdmission(input, async assertCurrent => {
        const op = begin(input, input.action, coordinatorId);
        const result = await driver.act(nativeCopy(input), assertCurrent); acknowledged(op, result);
        return { state: 'started', source: op.source, evidence: `${host}:${op.key}` };
      });
    },
    async lookup(input) {
      binding(input);
      const op = journal.operations.find(value => value.key === input.key && value.kind === input.action);
      if (!op || op.source !== input.source || op.checkpoint !== input.checkpoint) return { state: 'uncertain' };
      if (op.phase === 'completed') return { state: 'completed', source: op.source, evidence: `${host}:${op.key}`,
        ...(op.kind === 'compact' ? { checkpoint: op.checkpoint, resumed: true, acceptanceEvidence: op.acceptanceEvidence } : {}) };
      const result = await driver.lookup(nativeCopy(op));
      if (result?.completed !== true) return { state: result?.uncertain ? 'uncertain' : 'started', source: op.source, evidence: `${host}:${op.key}` };
      if (op.kind === 'review') { op.phase = 'completed'; persist(); return { state: 'completed', source: op.source, evidence: `${host}:${op.key}` }; }
      if (result.compacted !== true) return { state: 'uncertain' };
      if (op.phase === 'acknowledged') {
        await guard({ ...input, action: 'resume' });
        const { completion } = await withTaskAdmission(input, assertCurrent => {
          op.phase = 'resume-intent'; persist();
          // The driver submits resume synchronously; model completion must not hold the binding lock.
          return { completion: driver.resume(nativeCopy(op), assertCurrent) };
        });
        if ((await completion)?.resumed !== true) return { state: 'uncertain' };
        op.phase = 'resumed'; persist();
      }
      if (op.phase !== 'resumed' || typeof verifyResume !== 'function') return { state: 'uncertain' };
      const verified = await verifyResume({ ...context, coordinatorId, source: op.source, checkpoint: op.checkpoint });
      if (nativeText(verified?.acceptanceEvidence)) {
        op.phase = 'completed'; op.acceptanceEvidence = verified.acceptanceEvidence; persist();
        return { state: 'completed', source: op.source, checkpoint: op.checkpoint, resumed: true, evidence: `${host}:${op.key}`, acceptanceEvidence: verified.acceptanceEvidence };
      }
      return { state: 'started', source: op.source, evidence: `${host}:${op.key}` };
    }
  };
  const reconcileRelease = async workerId => {
    binding(context);
    const op = journal.operations.find(value => value.kind === 'release' && value.nativeId === workerId);
    if (!op) throw new Error('native release has no recorded attempt');
    if (op.released) return { released: true, duplicate: true };
    // Query the driver directly: the journal itself is not proof of native release.
    const actual = await driver.snapshot([workerId]);
    if (actual.workers?.some(worker => worker.id === workerId && worker.status === 'released' && worker.occupiesSlot === false)) {
      op.phase = 'acknowledged'; op.released = true; persist(); return { released: true };
    }
    return { released: false, uncertain: true };
  };
  return { workers: workerController(root, { session, task, preferencesFile }, workerHost),
    actions: actionController(root, { session, task, preferencesFile, bundle }, actionHost), workerHost, actionHost,
    capabilities: () => driver.capabilities(), nativeStatus: () => nativeCopy(journal),
    output: key => driver.output(key),
    reconcileRelease,
    async release(workerId) {
      await guard({ ...context, action: 'release', workerId });
      const prior = journal.operations.find(op => op.kind === 'release' && op.nativeId === workerId);
      if (prior) return reconcileRelease(workerId);
      return withTaskAdmission(context, async () => {
        if (driver.capabilities().release !== true || driver.canRelease?.(workerId) !== true) throw new Error('native work must settle and support release before recording intent');
        const op = begin({ ...context, releaseWorker: workerId }, 'release', workerId);
        const result = await driver.release(workerId);
        if (result?.released === false) {
          // Drivers return false only when no release effect was submitted.
          journal.operations.splice(journal.operations.indexOf(op), 1); persist(op.key);
          throw new Error('native slot release prerequisites changed');
        }
        if (result?.released !== true) throw new Error('native slot release unverified; reconcile before retry');
        op.phase = 'acknowledged'; op.released = true; persist(); return { released: true };
      });
    },
    close() { if (closed) return; closed = true; try { driver.close(); } finally { owner.close(); } }
  };
}
