import { nativeActions, bundleRoot } from '../platform/index.mjs';
import { forwardTelemetry } from '../application/telemetry/observe.mjs';

// Trusted callbacks run outside native locks. Rust owns claims, validation and persistence.
export function actionController(root, { session, task, preferencesFile, bundle = bundleRoot() } = {}, host = {}) {
  const call = (phase, input = {}) => {
    const response = nativeActions({ root, session, task, preferencesFile, bundle, phase, ...input });
    forwardTelemetry(response.export);
    if (response.error) throw new Error(response.error);
    return response.value;
  };
  call('open');
  const supported = ['authorize', 'observe', 'execute'].every(name => typeof host[name] === 'function');
  const unsupported = () => ({ supported: false, executed: false, reason: 'A trusted host action adapter is unavailable; retain advice/manual execution.' });
  const settle = (id, receipt, result) => {
    try { return call('finish', { id, receipt, result }); }
    catch {
      try { return { ...call('finish', { id, receipt, result: { state: 'uncertain' } }), uncertain: true }; }
      catch { return { supported: true, executed: false, uncertain: true, recoveryKey: receipt.key, reason: 'Native outcome could not be persisted; do not repeat the action before reconciliation.' }; }
    }
  };
  const execute = async id => {
    if (!supported) return unsupported();
    const offer = call('find', { id });
    if (offer.executionReceipt) return { supported: true, duplicate: true, executed: offer.executionReceipt.state === 'completed', receipt: offer.executionReceipt };
    const authority = await host.authorize({ root, session, task, offer: structuredClone(offer) });
    if (authority?.allowed !== true) return { supported: true, executed: false, reason: 'Host did not verify current scoped action authority.' };
    const observation = await host.observe({ root, session, task, action: offer.action });
    const claimed = call('claim', { id, observation });
    if (claimed.duplicate) return { supported: true, duplicate: true, executed: claimed.receipt.state === 'completed', receipt: claimed.receipt };
    try {
      const result = await host.execute({ root, session, task, key: claimed.receipt.key, action: claimed.offer.action, scope: claimed.offer.scope,
        source: claimed.receipt.source, taskContract: claimed.offer.taskContract ?? null, checkpoint: claimed.receipt.checkpoint });
      return settle(id, claimed.receipt, result);
    } catch { return settle(id, claimed.receipt, { state: 'uncertain' }); }
  };
  const reconcile = async id => {
    if (typeof host.lookup !== 'function') return unsupported();
    const offer = call('find', { id }), receipt = offer.executionReceipt;
    if (!receipt) throw new Error('action has no recorded execution attempt');
    if (['completed', 'not-started'].includes(receipt.state)) return { supported: true, executed: receipt.state === 'completed', receipt };
    const result = await host.lookup({ root, session, task, key: receipt.key, action: offer.action, source: receipt.source, checkpoint: receipt.checkpoint });
    return settle(id, receipt, result);
  };
  return { supported, execute, reconcile };
}
