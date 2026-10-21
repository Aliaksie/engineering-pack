import { assertNativeSource, managedControls, nativeText as text, nativeCopy as copy } from '../managed-control.mjs';
import { realRoot } from '../../platform/index.mjs';

const readTools = ['Read', 'Glob', 'Grep'];
const deniedTools = ['Bash', 'Edit', 'Write', 'NotebookEdit', 'Agent', 'Task', 'TaskOutput', 'TaskStop', 'mcp__*'];

// The consumer supplies its installed official Agent SDK query function. The portable
// package neither installs an SDK nor imports a sibling/global npm package implicitly.
export function createClaudeControls({ query, coordinatorId, sdkOptions = {}, startTimeoutMs = 30000, ...options } = {}) {
  if (typeof query !== 'function' || coordinatorId !== options.session || !text(coordinatorId)
    || !Number.isSafeInteger(startTimeoutMs) || startTimeoutMs < 1 || startTimeoutMs > 300000
    || !sdkOptions || typeof sdkOptions !== 'object' || Array.isArray(sdkOptions)
    || Object.keys(sdkOptions).some(key => !['env', 'pathToClaudeCodeExecutable', 'plugins', 'settingSources', 'maxTurns', 'maxBudgetUsd', 'stderr'].includes(key))) throw new Error('explicit Claude SDK query, real session identity and bounded startup configuration required');
  options.root = realRoot(options.root);
  const sessions = new Map(), jobs = new Map(), cancellations = new Map(); let closed = false;
  const launch = (input, { resume, prompt, remember = () => {}, kind, assertCurrent = () => {} }) => {
    if (closed || (resume && sessions.get(resume)?.job && !sessions.get(resume).job.settled)) throw new Error('Claude native session is busy or unavailable');
    const job = { key: input.key, kind, settled: false, uncertain: false, status: 'running', compacted: false, chunks: [], bytes: 0, truncated: false };
    jobs.set(input.key, job);
    let accept, reject, timer;
    const started = new Promise((resolve, fail) => { accept = resolve; reject = fail; timer = setTimeout(() => { job.uncertain = true; fail(new Error('Claude start acknowledgement missing; retain intent')); }, startTimeoutMs); });
    started.catch(() => {});
    job.clearStart = () => clearTimeout(timer);
    job.failStart = error => { clearTimeout(timer); reject(error); };
    let permit;
    const gate = new Promise(resolve => { permit = resolve; });
    async function* compactInput() {
      if (await gate) yield { type: 'user', message: { role: 'user', content: prompt }, parent_tool_use_id: null, session_id: resume ?? '' };
    }
    let stream;
    try { stream = query({ prompt: kind === 'compact' ? compactInput() : prompt, options: { ...sdkOptions, cwd: options.root, ...(resume ? { resume } : {}),
      ...(input.model ? { model: input.model } : {}), tools: readTools, allowedTools: readTools, disallowedTools: deniedTools,
      permissionMode: 'dontAsk', mcpServers: {}, canUseTool: async () => ({ behavior: 'deny', message: 'Only the bounded read-only tool set is available.' }) } });
    } catch (error) { clearTimeout(timer); permit(false); reject(error); throw error; }
    if (!stream || typeof stream[Symbol.asyncIterator] !== 'function') { clearTimeout(timer); reject(new Error('Claude SDK did not return a native stream')); throw new Error('Claude native stream unavailable'); }
    job.stream = stream;
    job.done = (async () => {
      if (kind === 'compact') {
        if (typeof stream.supportedCommands !== 'function' || !(await stream.supportedCommands()).some(command => command.name === 'compact')) throw new Error('Claude does not advertise native compaction');
        assertCurrent();
        assertNativeSource(options.root, input.source, options.session, options.task);
        permit(true);
      }
      let state;
      for await (const message of stream) {
        if (closed) throw new Error('Claude owner closed before completion');
        if (message.type === 'system' && message.subtype === 'init') {
          if (!text(message.session_id) || !text(message.model) || (resume && message.session_id !== resume)) throw new Error('Claude native session/model identity changed');
          if (!resume && sessions.has(message.session_id)) throw new Error('Claude reused a native worker identity');
          remember(message.session_id);
          state = { id: message.session_id, model: message.model, job }; sessions.set(state.id, state); job.nativeId = state.id;
          if (input.model && message.model !== input.model) throw new Error('Claude native model differs from its configured mapping');
          clearTimeout(timer); accept();
        }
        if (message.type === 'system' && message.subtype === 'compact_boundary' && kind === 'compact' && message.session_id === job.nativeId) job.compacted = true;
        if (message.type === 'assistant' && state) {
          if (message.session_id !== state.id) throw new Error('Claude assistant message belongs to another session');
          if (text(message.message?.model)) state.model = message.message.model;
          for (const block of message.message?.content ?? []) if (block.type === 'text' && typeof block.text === 'string') {
            const size = new TextEncoder().encode(block.text).length;
            if (job.bytes + size <= 65536 && job.chunks.length < 256) { job.chunks.push(block.text); job.bytes += size; }
            else job.truncated = true;
          }
        }
        if (message.type === 'result') {
          if (!state || message.session_id !== state.id) throw new Error('Claude result is not bound to the initialized session');
          job.settled = true; job.uncertain = false;
          job.status = message.subtype === 'success' && message.is_error !== true ? 'idle' : cancellationsHas(state.id) ? 'cancelled' : 'failed';
          job.result = { subtype: message.subtype, isError: message.is_error === true,
            usage: Object.fromEntries(['input_tokens', 'output_tokens', 'cache_read_input_tokens', 'cache_creation_input_tokens'].map(key => [key,
              Number.isSafeInteger(message.usage?.[key]) && message.usage[key] >= 0 ? message.usage[key] : null])),
            durationMs: Number.isSafeInteger(message.duration_ms) && message.duration_ms >= 0 ? message.duration_ms : null,
            totalCostUsd: Number.isFinite(message.total_cost_usd) && message.total_cost_usd >= 0 ? message.total_cost_usd : null };
        }
      }
      if (!job.settled) throw new Error('Claude stream ended without a native result');
    })().catch(error => { permit(false); job.uncertain = true; job.status = 'unknown'; clearTimeout(timer); reject(error); try { stream.close?.(); } catch { /* Retained intent owns uncertainty. */ } });
    return { job, started };
  };
  const cancellationsHas = id => [...cancellations.values()].includes(id);
  const value = state => ({ workerId: state.id, model: state.model, status: closed || state.job.uncertain ? 'unknown' : state.job.status,
    occupiesSlot: !state.released, evidence: `claude:${state.id}:${state.job.key.slice(0, 24)}` });
  const driver = {
    capabilities: () => ({ workers: true, review: true, compaction: true, cancellation: true, release: true, protocol: 'official Agent SDK query stream' }),
    async snapshot(ids) {
      return { allowed: !closed, workers: ids.flatMap(id => sessions.has(id) ? [{ id, ...Object.fromEntries(Object.entries(value(sessions.get(id))).filter(([key]) => key !== 'workerId')),
        ...(sessions.get(id).released ? { status: 'released' } : {}) }] : []),
      settledCancellations: [...cancellations].filter(([, id]) => sessions.get(id)?.job.settled && !sessions.get(id).job.uncertain).map(([key]) => key) };
    },
    async start(input, remember) {
      if (input.reuseWorker && (!sessions.has(input.reuseWorker) || sessions.get(input.reuseWorker).released
        || sessions.get(input.reuseWorker).job.status !== 'idle' || sessions.get(input.reuseWorker).model !== input.model)) throw new Error('Claude reuse needs an observed idle mapped session');
      const { job, started } = launch(input, { resume: input.reuseWorker, remember, kind: 'worker', prompt: JSON.stringify({ goal: input.brief.goal,
        scope: input.scope, sources: input.brief.sources, revision: input.revision, acceptance: input.brief.acceptance,
        restrictions: input.brief.restrictions, returnShape: input.brief.returnShape }) });
      await started; return value(sessions.get(job.nativeId));
    },
    async cancel(input) {
      const state = sessions.get(input.workerId);
      if (!state?.job || state.job.settled || typeof state.job.stream.interrupt !== 'function') throw new Error('Claude native interrupt is unavailable for this active worker');
      cancellations.set(input.key, state.id); await state.job.stream.interrupt(); return value(state);
    },
    async act(input, assertCurrent) {
      const { started } = launch(input, { assertCurrent, resume: coordinatorId, kind: input.action, prompt: input.action === 'compact' ? '/compact'
        : `Review only this authorized scope: ${input.scope}. Source identity: ${input.source}. Return findings and evidence. Do not write, delegate or publish.` });
      await started; return {};
    },
    async lookup(op) {
      const job = jobs.get(op.key);
      return job ? { completed: job.settled && job.status === 'idle' && !job.uncertain, compacted: job.compacted,
        uncertain: job.uncertain || (job.settled && job.status !== 'idle') } : { uncertain: true };
    },
    async resume(op) {
      const resumed = launch({ key: `${op.key}:resume` }, { resume: coordinatorId, kind: 'resume', prompt:
        `Resume the current task from checkpoint ${op.checkpoint}, bound to source ${op.source}. Verify retained acceptance and constraints; report missing evidence. Do not edit, delegate or publish.` });
      await resumed.started; await resumed.job.done;
      return { resumed: resumed.job.settled && resumed.job.status === 'idle' && !resumed.job.uncertain };
    },
    output(key) {
      const job = jobs.get(key);
      return job ? { chunks: copy(job.chunks), truncated: job.truncated, nativeResult: copy(job.result ?? null), semanticVerdict: 'not-judged' } : { unavailable: true };
    },
    canRelease: id => Boolean(sessions.get(id)?.job?.settled && !sessions.get(id).job.uncertain && typeof sessions.get(id).job.stream.close === 'function'),
    async release(id) {
      const state = sessions.get(id);
      if (!state?.job?.settled || state.job.uncertain || typeof state.job.stream.close !== 'function') return { released: false };
      await state.job.stream.close(); state.released = true; return { released: true };
    },
    close() {
      closed = true;
      for (const job of jobs.values()) { job.failStart(new Error('Claude owner closed before acknowledgement')); try { job.stream.close?.(); } catch { /* Retained journal never infers release from cleanup. */ } }
    }
  };
  return managedControls({ ...options, host: 'claude', coordinatorId, driver });
}
