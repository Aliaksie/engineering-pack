import { decide, digest } from '../engine-client.mjs';
import { readProject } from '../project/configuration.mjs';
import { activeTask, bindingFile, readSessionState, updateSessionState } from './activity.mjs';
import { sessionStatus } from './status.mjs';
import { saveCheckpoint } from '../checkpoint.mjs';
import { clock, repositoryState, lockState, releaseState } from '../../platform/index.mjs';

// This is an explicit application API, not an executable-adapter declaration in project JSON.
export function actionController(root, { session, task, preferencesFile, bundle } = {}, host = {}) {
  decide('activity-identity', { session, task });
  const supported = ['authorize', 'observe', 'execute'].every(name => typeof host[name] === 'function');
  const unsupported = () => ({ supported: false, executed: false, reason: 'A trusted host action adapter is unavailable; retain advice/manual execution.' });
  const find = id => {
    if (!/^[a-f0-9]{64}$/.test(id ?? '')) throw new Error('invalid action proposal id');
    const state = readSessionState(root, session, task);
    if (!Object.hasOwn(state.recommendations ?? {}, id)) throw new Error('unknown action proposal');
    return state.recommendations[id];
  };
  const finish = (id, receipt, result) => {
    let response;
    updateSessionState(root, session, task, state => {
      const completed = decide('action-finish', { state, id, key: receipt.key, generation: receipt.generation, result });
      Object.assign(state, completed.state); response = completed;
      if (response.receipt.state === 'completed') {
        const action = state.recommendations[id].action;
        Object.assign(state, decide('feature-record', { state, source: receipt.source, at: clock().iso, provenance: 'runtime-observed',
          input: { feature: action === 'compact' ? 'compaction' : 'review', status: 'used', evidence: response.receipt.evidence,
            reason: 'Host adapter reported the scoped action; delivery acceptance remains a separate task receipt.' } }).state);
      }
    });
    return { supported: true, executed: response.receipt.state === 'completed', uncertain: response.receipt.state === 'uncertain', receipt: response.receipt, ignoredStaleResult: response.ignored };
  };
  const settle = (id, receipt, result) => {
    try { return finish(id, receipt, result); }
    catch {
      try { return { ...finish(id, receipt, { state: 'uncertain' }), uncertain: true }; }
      catch { return { supported: true, executed: false, uncertain: true, recoveryKey: receipt.key, reason: 'Native outcome could not be persisted; do not repeat the action before reconciliation.' }; }
    }
  };
  const execute = async id => {
    if (!supported) return unsupported();
    const offer = find(id);
    if (offer.executionReceipt) return { supported: true, duplicate: true, executed: offer.executionReceipt.state === 'completed', receipt: offer.executionReceipt };
    const authority = await host.authorize({ root, session, task, offer: JSON.parse(JSON.stringify(offer)) });
    if (authority?.allowed !== true) return { supported: true, executed: false, reason: 'Host did not verify current scoped action authority.' };
    const observation = await host.observe({ root, session, task, action: offer.action });
    const observedAt = Date.parse(observation?.observedAt), now = clock().ms;
    if (observation?.sessionId !== session || !Number.isFinite(observedAt) || observedAt > now || now - observedAt > 30000) throw new Error('fresh matching action observations are required');
    decide('action-facts', { facts: observation.facts });
    const ticket = lockState(bindingFile(root, session));
    if (!ticket) throw new Error('active task writer is busy');
    let claimed;
    try {
      if (activeTask(root, session) !== task) throw new Error('action execution requires the active coordinator task');
      const project = readProject(root, { preferencesFile }), repository = repositoryState(root);
      const status = sessionStatus(observation.facts.observations ?? {}, project, { root, session, task, bundle });
      let checkpoint = null;
      if (offer.action === 'compact') {
        // A snapshot refresh cannot renew stale continuation; action-check still verifies it.
        const saved = saveCheckpoint(root, session, { taskId: task }, { expectedSource: offer.source });
        checkpoint = digest(saved.state);
      }
      updateSessionState(root, session, task, state => {
        const result = decide('action-claim', { state, input: { id }, project, repository, status, facts: observation.facts, session, now: clock().ms, checkpoint });
        Object.assign(state, result.state); claimed = result;
      });
    } finally { releaseState(ticket); }
    if (claimed.duplicate) return { supported: true, duplicate: true, executed: claimed.receipt.state === 'completed', receipt: claimed.receipt };
    try {
      const result = await host.execute({ root, session, task, key: claimed.receipt.key, action: offer.action, scope: offer.scope,
        source: claimed.receipt.source, checkpoint: claimed.receipt.checkpoint });
      return settle(id, claimed.receipt, result);
    } catch { return settle(id, claimed.receipt, { state: 'uncertain' }); }
  };
  const reconcile = async id => {
    if (typeof host.lookup !== 'function') return unsupported();
    const offer = find(id), receipt = offer.executionReceipt;
    if (!receipt) throw new Error('action has no recorded execution attempt');
    if (['completed', 'not-started'].includes(receipt.state)) return { supported: true, executed: receipt.state === 'completed', receipt };
    const result = await host.lookup({ root, session, task, key: receipt.key, action: offer.action, source: receipt.source, checkpoint: receipt.checkpoint });
    return settle(id, receipt, result);
  };
  return { supported, execute, reconcile };
}
