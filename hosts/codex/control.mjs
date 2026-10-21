import { decide, digest } from '../../application/engine-client.mjs';
import { taskRepository } from '../../application/session/activity.mjs';
import { withTaskAdmission } from '../../application/session/admission.mjs';
import { workerController } from '../../application/session/workers.mjs';
import { actionController } from '../../application/session/actions.mjs';
import { clock, realRoot, rpcProcess, runtimeVersion, stateFile, readState, writeState, lockState, releaseState } from '../../platform/index.mjs';

const text = value => typeof value === 'string' && value.length > 0 && value.length <= 512 && !/[\x00-\x1f\x7f]/.test(value);
const hash = value => typeof value === 'string' && /^[a-f0-9]{64}$/.test(value);
const terminal = value => ['completed', 'failed', 'interrupted'].includes(value);
const copy = value => JSON.parse(JSON.stringify(value));
const limit = 262144;

// App-server is an explicit, exclusive client surface, not a bridge into a running desktop.
// The owner allocates a bounded lane; this number is not a provider/account limit.
export function createCodexControls({ root, session, task, threadId, capacity, client, authorize,
  observations, verifyResume, preferencesFile, bundle } = {}) {
  root = realRoot(root);
  if (![session, task, threadId].every(text) || !Number.isSafeInteger(capacity) || capacity < 1 || capacity > 64
    || !client || !['request', 'subscribe'].every(name => typeof client[name] === 'function') || typeof authorize !== 'function') throw new Error('explicit Codex client, coordinator identity, lane capacity and authority verifier are required');
  const tree = digest([root, session, task]);
  const file = stateFile(root, `codex-control-${tree.slice(0, 24)}.json`, true, session);
  const owner = lockState(file);
  if (!owner) throw new Error('Codex task control already has an owner; do not bypass an unresolved owner lock');
  let journal, closed = false, broken = false, unsubscribe;
  try {
    journal = readState(file, limit) ?? { formatVersion: 1, tree, threadId, operations: [] };
    if (Object.keys(journal).some(key => !['formatVersion', 'tree', 'threadId', 'operations'].includes(key))
      || journal.formatVersion !== 1 || journal.tree !== tree || journal.threadId !== threadId || !Array.isArray(journal.operations)
      || journal.operations.length > 192 || new Set(journal.operations.map(op => op?.key)).size !== journal.operations.length
      || journal.operations.some(op => !op || Object.keys(op).some(key => !['key', 'kind', 'source', 'checkpoint', 'threadId', 'turnId', 'model', 'phase', 'outcome', 'compacted', 'evidence'].includes(key))
        || !hash(op.key) || !['worker', 'cancel', 'review', 'compact'].includes(op.kind)
        || !['intent', 'acknowledged', 'resume-intent', 'resumed'].includes(op.phase)
        || (op.threadId !== null && !text(op.threadId)) || (op.turnId !== null && !text(op.turnId))
        || (op.source !== null && !hash(op.source)) || (op.checkpoint !== null && !hash(op.checkpoint))
        || (op.model !== null && !text(op.model)) || (op.outcome !== null && !terminal(op.outcome))
        || typeof op.compacted !== 'boolean' || (op.evidence !== null && !text(op.evidence)))) throw new Error('invalid bounded native operation journal');
  } catch (error) { releaseState(owner); throw error; }
  const persist = () => {
    if (closed || broken) throw new Error('native journal owner is unavailable');
    try { writeState(file, journal, limit); } catch (error) { broken = true; throw error; }
  };
  const usable = () => { if (closed || broken) throw new Error('native controller unavailable; reconcile retained intent before new effects'); };
  const context = { root, session, task, tree };
  const checkContext = input => {
    if (input.root !== root || input.session !== session || input.task !== task || (input.tree !== undefined && input.tree !== tree)) throw new Error('native controller coordinator binding changed');
  };
  const authority = async input => { checkContext(input); usable(); return authorize(copy(input)); };
  const guard = async input => {
    const value = await authority(input);
    if (value?.allowed !== true || !text(value.evidence)) throw new Error('native effect requires freshly verified scoped authority');
    if (input.source !== undefined && decide('task-source', { repository: taskRepository(root, session, task) }) !== input.source) throw new Error('native effect source changed after admission');
  };
  const begin = (input, kind, nativeThread = null) => {
    usable(); checkContext(input);
    if (!hash(input.key)) throw new Error('invalid native operation key');
    const prior = journal.operations.find(op => op.key === input.key);
    if (prior) throw new Error('native operation already attempted; reconcile its original key');
    if (journal.operations.length >= 192) throw new Error('native operation journal capacity reached');
    const op = { key: input.key, kind, source: input.source ?? null, checkpoint: input.checkpoint ?? null,
      threadId: nativeThread, turnId: null, model: null, phase: 'intent', outcome: null, compacted: false, evidence: null };
    journal.operations.push(op); persist(); return op;
  };
  const native = async (method, params) => { usable(); return client.request(method, params); };
  const readThread = async id => {
    const result = await native('thread/read', { threadId: id, includeTurns: false });
    if (result?.thread?.id !== id || !['active', 'idle', 'notLoaded', 'systemError'].includes(result.thread.status?.type)) throw new Error('native thread observation is unavailable');
    return result.thread;
  };
  const workers = () => [...new Set(journal.operations.filter(op => op.kind === 'worker' && op.threadId).map(op => op.threadId))];
  const latestWorker = id => journal.operations.findLast(op => op.kind === 'worker' && op.threadId === id);
  const receipt = op => `codex:${op.threadId ?? 'unknown'}:${op.turnId ?? op.key.slice(0, 24)}`;
  const workerValue = (op, thread) => {
    const settled = op.phase === 'acknowledged' && op.turnId && terminal(op.outcome);
    const released = settled && thread.status.type === 'notLoaded';
    return { workerId: thread.id, status: released ? 'released' : !settled || thread.status.type === 'active' ? 'running'
      : op.outcome === 'failed' ? 'failed' : op.outcome === 'interrupted' ? 'cancelled' : 'idle', occupiesSlot: !released,
    model: text(thread.model) ? thread.model : null, evidence: receipt(op) };
  };
  // Notifications can precede their request response. Capture them before sending effects;
  // apply only IDs belonging to our own recorded operations, never transcript guesses.
  const early = new Map();
  const apply = (op, message) => {
    const p = message.params;
    if (message.method === 'turn/completed' && p?.threadId === op.threadId && p.turn?.id === op.turnId && terminal(p.turn.status)) {
      op.outcome = p.turn.status; op.evidence = receipt(op); return true;
    }
    if (message.method === 'item/completed' && op.kind === 'compact' && p?.threadId === op.threadId && p.item?.type === 'contextCompaction') {
      if (op.turnId && op.turnId !== p.turnId) return false;
      op.turnId = p.turnId; op.compacted = true; return true;
    }
    return false;
  };
  try { unsubscribe = client.subscribe(message => {
    if (message.method === '$connection/closed') { broken = true; return; }
    if (!['turn/completed', 'item/completed'].includes(message.method)) return;
    try {
      let changed = false;
      for (const op of journal.operations) {
        if (op.phase === 'intent' && op.threadId === message.params?.threadId && !op.turnId) {
          if (early.size >= 128) throw new Error('native early event capacity reached');
          const key = `${message.method}:${message.params.turnId ?? message.params.turn?.id}:${message.params.item?.id ?? ''}`;
          early.set(key, copy(message));
        }
        if (apply(op, message)) changed = true;
      }
      if (changed) persist();
    } catch { broken = true; }
  });
  if (typeof unsubscribe !== 'function') throw new Error('native client subscription must return an unsubscribe function');
  } catch (error) { releaseState(owner); throw error; }
  const acknowledge = (op, turn) => {
    if (!text(turn?.id) || !['inProgress', 'completed', 'failed', 'interrupted'].includes(turn.status)) throw new Error('native turn acknowledgement is unavailable');
    op.turnId = turn.id; op.phase = 'acknowledged'; op.outcome = terminal(turn.status) ? turn.status : null;
    for (const [key, message] of early) if (message.params?.threadId === op.threadId) { apply(op, message); early.delete(key); }
    persist();
  };
  const refresh = async op => {
    if (!op.threadId || !op.turnId || terminal(op.outcome)) return;
    // Bounded metadata-only pagination works with persisted paginated thread history.
    const result = await native('thread/turns/list', { threadId: op.threadId, limit: 64, itemsView: 'summary', sortDirection: 'desc' });
    const turn = result?.data?.find(value => value.id === op.turnId);
    if (turn && terminal(turn.status)) { op.outcome = turn.status; op.evidence = receipt(op); persist(); }
  };
  const inventory = async input => {
    checkContext(input); usable();
    const loaded = await native('thread/loaded/list', { limit: 256 });
    if (!Array.isArray(loaded?.data) || loaded.nextCursor || loaded.data.length > 256 || loaded.data.some(id => !text(id))) throw new Error('complete bounded native thread inventory is unavailable');
    if (!loaded.data.includes(threadId)) throw new Error('coordinator thread must be loaded by the owning client');
    if ((await readThread(threadId)).sessionId !== session) throw new Error('native coordinator session identity changed');
    const values = [];
    for (const id of workers()) {
      const op = latestWorker(id); await refresh(op);
      const thread = await readThread(id);
      const { workerId, ...value } = workerValue(op, thread); values.push({ id: workerId, ...value });
    }
    const own = new Set([threadId, ...workers()]);
    const occupied = values.filter(worker => worker.occupiesSlot).length;
    const other = loaded.data.filter(id => !own.has(id)).length;
    if (1 + occupied + other > capacity) throw new Error('native lane is over capacity; no new admission');
    const lookups = [];
    for (const op of journal.operations) {
      if (!['worker', 'cancel'].includes(op.kind) || op.phase !== 'acknowledged' || !op.turnId) continue;
      if (op.kind === 'cancel') { await refresh(op); if (!terminal(op.outcome)) continue; }
      if (values.some(worker => worker.id === op.threadId)) lookups.push({ key: op.key, status: 'started', workerId: op.threadId, settled: true, evidence: receipt(op) });
    }
    return { host: 'codex', capacity, occupiedSlots: 1 + occupied + other, taskWorkers: occupied, allowed: true,
      sessionId: session, observedAt: clock().iso, source: 'codex-app-server:exclusive-lane-and-loaded-threads', workers: values, requests: lookups };
  };
  const workerHost = { authorize: authority, observe: inventory,
    async dispatch(input) {
      await guard({ ...input, action: 'dispatch' });
      const snapshot = await inventory(input);
      if (!input.reuseWorker && snapshot.occupiedSlots >= snapshot.capacity) throw new Error('native lane has no available slot');
      if (input.reuseWorker && !snapshot.workers.some(worker => worker.id === input.reuseWorker && worker.status === 'idle' && worker.model === input.model)) throw new Error('native worker is not reusable');
      return withTaskAdmission(input, async assertCurrent => {
        const op = begin(input, 'worker', input.reuseWorker ?? null);
        if (input.reuseWorker) {
          op.model = snapshot.workers.find(worker => worker.id === input.reuseWorker).model;
          persist();
        }
        if (!input.reuseWorker) {
          // A fresh thread avoids inheriting the coordinator transcript. Permission escalation
          // and built-in unmetered subagents are disabled for this adapter's worker lane.
          const result = await native('thread/start', { cwd: root, model: input.model, sandbox: 'read-only', approvalPolicy: 'never',
            config: { 'features.multi_agent': false, 'features.multi_agent_v2': false }, developerInstructions: 'Perform only the supplied bounded assignment. Do not delegate, write files, or perform remote writes.' });
          if (!text(result?.thread?.id) || !text(result.model)) throw new Error('native worker/model acknowledgement is unavailable');
          op.threadId = result.thread.id; op.model = result.model; persist();
          if (result.model !== input.model) throw new Error('native worker model differs from the approved mapping');
          if (result.sandbox?.type !== 'readOnly' || result.sandbox.networkAccess === true || result.approvalPolicy !== 'never') throw new Error('native worker permissions differ from the read-only lane');
        }
        assertCurrent();
        const response = await native('turn/start', { threadId: op.threadId, model: input.model,
          input: [{ type: 'text', text: JSON.stringify({ goal: input.brief.goal, scope: input.scope, sources: input.brief.sources,
            revision: input.revision, acceptance: input.brief.acceptance, restrictions: input.brief.restrictions, returnShape: input.brief.returnShape }) }] });
        acknowledge(op, response.turn);
        // Startup is observed through the persisted thread/model and turn acknowledgements.
        // An immediate history read is not required to certify that native effect. Keep the
        // slot occupied; reconciliation still verifies current thread metadata before reuse.
        return { workerId: op.threadId, model: op.model,
          status: op.outcome === 'completed' ? 'idle' : op.outcome === 'failed' ? 'failed' : op.outcome === 'interrupted' ? 'cancelled' : 'running',
          occupiesSlot: true, evidence: receipt(op) };
      });
    },
    async cancel(input) {
      await guard({ ...input, action: 'cancel' });
      const worker = latestWorker(input.workerId);
      if (!worker?.turnId) throw new Error('native cancellation needs a known turn');
      return withTaskAdmission(input, async () => {
        const op = begin(input, 'cancel', input.workerId); op.turnId = worker.turnId; persist();
        await native('turn/interrupt', { threadId: op.threadId, turnId: op.turnId });
        op.phase = 'acknowledged'; persist(); await refresh(op);
        if (!terminal(op.outcome)) throw new Error('native cancellation is acknowledged but its turn has not settled');
        return workerValue(op, await readThread(op.threadId));
      });
    }
  };
  const actionResult = op => ({ state: op.outcome === 'completed' && op.kind === 'review' ? 'completed' : op.phase === 'intent' || ['failed', 'interrupted'].includes(op.outcome) ? 'uncertain' : 'started',
    source: op.source, evidence: op.evidence ?? receipt(op) });
  const actionHost = { authorize: authority,
    async observe(input) {
      checkContext(input); usable();
      if (typeof observations !== 'function') throw new Error('native actions require actual fresh action/context observations');
      const value = await observations(copy(input));
      return { ...value, facts: { ...value.facts, observations: { ...value.facts?.observations,
        host: { ...value.facts?.observations?.host, name: 'codex', reliableCompaction: typeof verifyResume === 'function' } } } };
    },
    async execute(input) {
      await guard(input);
      if (!['review', 'compact'].includes(input.action) || (input.action === 'compact' && typeof verifyResume !== 'function')) throw new Error('native action is unsupported');
      if ((await readThread(threadId)).status.type !== 'idle') throw new Error('native coordinator must be idle before review/compaction');
      if (journal.operations.some(op => ['review', 'compact'].includes(op.kind) && !(op.kind === 'review' ? terminal(op.outcome) : op.phase === 'resumed'))) throw new Error('an unresolved coordinator action requires reconciliation');
      return withTaskAdmission(input, async () => {
        const op = begin(input, input.action, threadId);
        if (input.action === 'review') {
          const result = await native('review/start', { threadId, delivery: 'inline', target: { type: 'custom', instructions: `Review only this authorized scope: ${input.scope}. Source identity: ${input.source}. Report findings; do not change files or publish.` } });
          if (result.reviewThreadId !== threadId) throw new Error('native review returned a different thread');
          acknowledge(op, result.turn);
        } else {
          await native('thread/compact/start', { threadId }); op.phase = 'acknowledged';
          for (const [key, message] of early) if (message.params?.threadId === threadId) { apply(op, message); early.delete(key); }
          persist();
        }
        return actionResult(op);
      });
    },
    async lookup(input) {
      checkContext(input); usable();
      const op = journal.operations.find(value => value.key === input.key && value.kind === input.action);
      if (!op || op.source !== input.source || op.checkpoint !== input.checkpoint) return { state: 'uncertain' };
      await refresh(op);
      if (op.kind === 'compact' && op.compacted && op.outcome === 'completed') {
        if (op.phase === 'acknowledged') {
          await guard({ ...input, action: 'resume' });
          await withTaskAdmission(input, async () => {
            op.phase = 'resume-intent'; persist();
            const result = await native('thread/resume', { threadId, excludeTurns: true });
            if (result?.thread?.id !== threadId || result.thread.status?.type !== 'idle') throw new Error('native resume could not be verified');
            op.phase = 'resumed'; persist();
          });
        }
        if (op.phase === 'resumed' && typeof verifyResume === 'function') {
          const verified = await verifyResume(copy({ ...context, threadId, source: op.source, checkpoint: op.checkpoint }));
          if (text(verified?.acceptanceEvidence)) return { state: 'completed', source: op.source, checkpoint: op.checkpoint,
            evidence: receipt(op), resumed: true, acceptanceEvidence: verified.acceptanceEvidence };
        }
      }
      return actionResult(op);
    }
  };
  return { workers: workerController(root, { session, task, preferencesFile }, workerHost),
    actions: actionController(root, { session, task, preferencesFile, bundle }, actionHost), workerHost, actionHost,
    nativeStatus: () => copy(journal),
    capabilities: () => ({ workers: true, review: true, compaction: typeof verifyResume === 'function', cancellation: true, release: true, protocol: 'Codex App Server' }),
    async output(key, cursor = null) {
      const op = journal.operations.find(value => value.key === key);
      if (!op?.threadId || !op.turnId || (cursor !== null && (typeof cursor !== 'string' || cursor.length > 4096))) throw new Error('known native turn and bounded cursor required');
      const result = await native('thread/items/list', { threadId: op.threadId, turnId: op.turnId, limit: 32, ...(cursor === null ? {} : { cursor }) });
      if (!Array.isArray(result?.data) || result.data.length > 32) throw new Error('bounded native output page required');
      return { threadId: op.threadId, turnId: op.turnId, outcome: op.outcome, items: result.data, nextCursor: result.nextCursor ?? null };
    },
    async release(workerId) {
      const op = latestWorker(workerId);
      if (!op) throw new Error('unknown native worker');
      await guard({ ...context, action: 'release', workerId }); await refresh(op);
      if (!terminal(op.outcome)) throw new Error('native work must settle before unsubscribing');
      return withTaskAdmission(context, () => native('thread/unsubscribe', { threadId: workerId }));
    },
    close() { if (closed) return; closed = true; try { unsubscribe(); } finally { releaseState(owner); } }
  };
}

// Explicit entry point for an owned stdio app-server process. No model call is made here.
export async function openCodexControls({ processOptions, ...options } = {}) {
  if (!processOptions || !text(processOptions.command)) throw new Error('explicit Codex executable/process configuration required');
  const client = rpcProcess({ ...processOptions, args: [...(processOptions.args ?? []), 'app-server', '--listen', 'stdio://'] });
  let controls;
  try {
    controls = createCodexControls({ ...options, client });
    await client.request('initialize', { clientInfo: { name: 'craft_capabilities', version: runtimeVersion() } });
    client.notify('initialized', {});
    // Attach only the explicitly supplied coordinator; never discover the user's active UI.
    const result = await client.request('thread/resume', { threadId: options.threadId, excludeTurns: true });
    if (result?.thread?.id !== options.threadId || result.thread.sessionId !== options.session) throw new Error('coordinator resume identity mismatch');
    return { ...controls, async close() { try { controls.close(); } finally { await client.close(); } } };
  } catch (error) { controls?.close(); await client.close(); throw error; }
}
