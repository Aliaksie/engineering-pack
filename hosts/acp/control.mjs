import { assertNativeSource, managedControls, nativeText as text, nativeCopy as copy } from '../managed-control.mjs';
import { clock, realRoot, rpcProcess, runtimeVersion } from '../../platform/index.mjs';

export const ACP_HOSTS = { cursor: ['acp'], copilot: ['--acp', '--stdio'], gemini: ['--acp'], 'gemini-extension': ['--acp'] };
const selected = (state, category) => state.configOptions?.find(option => option.category === category && option.type === 'select');
const choices = options => options.flatMap(option => Array.isArray(option.options) ? choices(option.options) : [option.value]);
const capsule = input => JSON.stringify({ goal: input.brief.goal, scope: input.scope, sources: input.brief.sources, revision: input.revision,
  acceptance: input.brief.acceptance, restrictions: input.brief.restrictions, returnShape: input.brief.returnShape });

// ACP v1 is capability-negotiated. Optional v2 shapes are deliberately not inferred.
export function createAcpControls({ host, client, protocolVersion, agentCapabilities = {}, coordinatorState = {},
  coordinatorId, readOnlyMode, startTimeoutMs = 30000, ...options } = {}) {
  if (!Object.hasOwn(ACP_HOSTS, host) || protocolVersion !== 1 || !client || !['request', 'notify', 'subscribe'].every(k => typeof client[k] === 'function')
    || coordinatorId !== options.session || !text(coordinatorId) || !Number.isSafeInteger(startTimeoutMs) || startTimeoutMs < 1 || startTimeoutMs > 300000) throw new Error('owned ACP v1 client and matching real coordinator session required');
  options.root = realRoot(options.root);
  const sessions = new Map([[coordinatorId, { ...coordinatorState, id: coordinatorId, job: null }]]), jobs = new Map(), cancellations = new Map();
  let connected = true, controls, unsubscribe;
  const native = (method, params) => {
    if (!connected) return Promise.reject(new Error('ACP connection unavailable; do not replay effects'));
    return client.request(method, params);
  };
  const optionsFor = state => Array.isArray(state?.configOptions) ? state.configOptions : [];
  const metadata = (state, update) => {
    if (Array.isArray(update.configOptions)) state.configOptions = update.configOptions;
    if (Array.isArray(update.availableCommands)) state.availableCommands = update.availableCommands;
    if (update.modes) state.modes = update.modes;
  };
  const currentModel = state => selected(state, 'model')?.currentValue ?? null;
  const value = state => ({ workerId: state.id, model: currentModel(state), occupiesSlot: !state.released,
    status: state.released ? 'released' : !connected || !state.job || state.job.uncertain ? 'unknown' : state.job.status,
    evidence: `${host}:acp:${state.id}:${state.job?.key?.slice(0, 24) ?? 'unobserved'}` });
  const command = () => ['compact', 'compress'].find(name => sessions.get(coordinatorId).availableCommands?.some(entry => entry.name === name));
  const canResume = () => Boolean(agentCapabilities.sessionCapabilities?.resume || agentCapabilities.loadSession === true);
  const first = job => new Promise((resolve, reject) => {
    const timer = setTimeout(() => { job.uncertain = true; reject(new Error('ACP prompt start is unacknowledged; retain reservation')); }, startTimeoutMs);
    job.started = () => { clearTimeout(timer); resolve(); };
    job.startFailed = error => { clearTimeout(timer); reject(error); };
  });
  const launch = (state, input, prompt, kind) => {
    if (state.job && !state.job.settled) throw new Error('ACP session already has unresolved work');
    const job = { key: input.key, kind, status: 'running', settled: false, uncertain: false, compacted: false, chunks: [], bytes: 0, truncated: false };
    const started = first(job); started.catch(() => {});
    state.job = job; jobs.set(input.key, job);
    // The owned promise always consumes both success and failure, even if start times out.
    job.done = native('session/prompt', { sessionId: state.id, prompt: [{ type: 'text', text: prompt }] }).then(result => {
      if (!['end_turn', 'cancelled', 'max_tokens', 'max_turn_requests', 'refusal'].includes(result?.stopReason)) throw new Error('unknown ACP stop reason');
      job.settled = true; job.uncertain = false; job.stopReason = result.stopReason;
      job.status = result.stopReason === 'end_turn' ? 'idle' : result.stopReason === 'cancelled' ? 'cancelled' : 'failed'; job.started();
    }).catch(error => { job.uncertain = true; job.status = 'unknown'; job.startFailed(error); });
    return { job, started };
  };
  const select = async (state, category, requested) => {
    const option = selected({ configOptions: optionsFor(state) }, category);
    if (!option || !text(option.id) || !Array.isArray(option.options) || !choices(option.options).includes(requested)) throw new Error(`ACP does not advertise the requested ${category}`);
    metadata(state, await native('session/set_config_option', { sessionId: state.id, configId: option.id, value: requested }));
    if (selected(state, category)?.currentValue !== requested) throw new Error(`ACP ${category} acknowledgement differs from the requested value`);
  };
  const driver = {
    capabilities: () => ({ workers: text(readOnlyMode), review: true, compaction: Boolean(command() && canResume()),
      cancellation: true, release: Boolean(agentCapabilities.sessionCapabilities?.close), protocol: 'ACP v1' }),
    async snapshot(ids) {
      return { allowed: connected, workers: ids.flatMap(id => sessions.has(id) ? [{ id, ...Object.fromEntries(Object.entries(value(sessions.get(id))).filter(([k]) => k !== 'workerId')) }] : []),
        settledCancellations: [...cancellations].filter(([, id]) => sessions.get(id)?.job?.settled).map(([key]) => key) };
    },
    async start(input, remember, assertCurrent) {
      if (!text(readOnlyMode)) throw new Error('a verified read-only ACP mode must be configured');
      let state = input.reuseWorker ? sessions.get(input.reuseWorker) : null;
      if (input.reuseWorker && (!state || state.released || state.job?.status !== 'idle' || !state.job.settled || currentModel(state) !== input.model)) throw new Error('ACP worker is not an observed idle mapped session');
      if (!state) {
        const result = await native('session/new', { cwd: options.root, mcpServers: [] });
        if (!text(result?.sessionId) || sessions.has(result.sessionId)) throw new Error('ACP session identity unavailable or reused');
        remember(result.sessionId); state = { ...result, id: result.sessionId, job: null }; sessions.set(state.id, state);
      }
      if (selected(state, 'mode')) await select(state, 'mode', readOnlyMode);
      else {
        if (!state.modes?.availableModes?.some(mode => mode.id === readOnlyMode)) throw new Error('ACP does not advertise the requested read-only mode');
        await native('session/set_mode', { sessionId: state.id, modeId: readOnlyMode });
        state.modes.currentModeId = readOnlyMode;
      }
      await select(state, 'model', input.model);
      if ((selected(state, 'mode')?.currentValue ?? state.modes?.currentModeId) !== readOnlyMode) throw new Error('ACP mode changed while mapping the worker model');
      assertCurrent();
      assertNativeSource(options.root, input.source);
      const { started } = launch(state, input, capsule(input), 'worker');
      await started; return value(state);
    },
    async cancel(input) {
      const state = sessions.get(input.workerId);
      if (!state?.job || state.job.settled) throw new Error('ACP cancellation requires a known active prompt');
      cancellations.set(input.key, state.id);
      client.notify('session/cancel', { sessionId: state.id });
      // Cancellation is a notification. Its effect is established by the prompt result.
      return value(state);
    },
    async act(input) {
      const state = sessions.get(coordinatorId);
      if (state.job && !state.job.settled) throw new Error('ACP coordinator is busy or uncertain');
      const compact = input.action === 'compact' ? command() : null;
      if (input.action === 'compact' && (!compact || !canResume())) throw new Error('ACP compaction/resume is not advertised');
      launch(state, input, compact ? `/${compact}` : `Review only this authorized scope: ${input.scope}. Source: ${input.source}. Return findings and evidence. Do not edit, delegate or publish.`, input.action);
      return {};
    },
    async lookup(op) {
      const job = jobs.get(op.key);
      return job ? { completed: job.settled && job.stopReason === 'end_turn', compacted: job.compacted,
        uncertain: job.uncertain || (job.settled && job.stopReason !== 'end_turn') } : { uncertain: true };
    },
    async resume() {
      if (!canResume()) return { resumed: false };
      metadata(sessions.get(coordinatorId), await native(agentCapabilities.sessionCapabilities?.resume ? 'session/resume' : 'session/load', { sessionId: coordinatorId, cwd: options.root, mcpServers: [] }));
      return { resumed: true };
    },
    output(key) {
      const job = jobs.get(key);
      return job ? { chunks: copy(job.chunks), truncated: job.truncated, stopReason: job.stopReason ?? null, semanticVerdict: 'not-judged' } : { unavailable: true };
    },
    canRelease: id => Boolean(agentCapabilities.sessionCapabilities?.close && sessions.get(id)?.job?.settled && !sessions.get(id).job.uncertain),
    async release(id) {
      const state = sessions.get(id);
      if (!agentCapabilities.sessionCapabilities?.close || !state?.job?.settled) return { released: false };
      await native('session/close', { sessionId: id }); state.released = true; return { released: true };
    },
    close() {
      connected = false;
      for (const job of jobs.values()) if (!job.settled) { job.uncertain = true; job.startFailed(new Error('ACP owner closed before native completion')); }
      unsubscribe?.();
    }
  };
  try {
    unsubscribe = client.subscribe(message => {
      if (message.method === '$connection/closed') { connected = false; return; }
      if (message.method !== 'session/update') return;
      const state = sessions.get(message.params?.sessionId), update = message.params?.update;
      if (!state || !update) return;
      if (update.sessionUpdate === 'config_option_update' || update.sessionUpdate === 'available_commands_update') metadata(state, update);
      if (update.sessionUpdate === 'current_mode_update' && state.modes && text(update.currentModeId)) state.modes.currentModeId = update.currentModeId;
      if (update.sessionUpdate === 'usage_update' && Number.isSafeInteger(update.used) && update.used >= 0 && Number.isSafeInteger(update.size) && update.size > 0) {
        state.usage = { kind: 'current-context', currentTokens: update.used, capacityTokens: update.size, sessionId: state.id, observedAt: clock().iso, source: `${host}:acp.usage_update` };
      }
      const job = state.job;
      if (!job || job.settled) return;
      if (['agent_message_chunk', 'agent_thought_chunk', 'tool_call', 'tool_call_update'].includes(update.sessionUpdate)) job.started();
      if (update.sessionUpdate === 'compaction_update' && update.status === 'completed' && text(update.compactionId) && job.kind === 'compact') job.compacted = true;
      if (update.sessionUpdate === 'agent_message_chunk' && update.content?.type === 'text' && typeof update.content.text === 'string') {
        const size = new TextEncoder().encode(update.content.text).length;
        if (job.bytes + size <= 65536 && job.chunks.length < 256) { job.chunks.push(update.content.text); job.bytes += size; }
        else job.truncated = true;
      }
    });
    if (typeof unsubscribe !== 'function') throw new Error('ACP subscription must return cleanup');
    const external = options.observations;
    controls = managedControls({ ...options, host, coordinatorId, driver, observations: typeof external === 'function' ? async input => {
      const result = await external(input), usage = sessions.get(coordinatorId).usage;
      return { ...result, facts: { ...result.facts, observations: { ...result.facts?.observations,
        ...(result.facts?.observations?.usage || !usage ? {} : { usage }) } } };
    } : undefined });
    return { ...controls, restoreCoordinator: () => driver.resume() };
  } catch (error) { driver.close(); throw error; }
}

export async function openAcpControls({ processOptions, host, authenticate, ...options } = {}) {
  if (!Object.hasOwn(ACP_HOSTS, host) || !processOptions || !text(processOptions.command)) throw new Error('explicit supported ACP executable required');
  const client = rpcProcess({ ...processOptions, timeoutMs: processOptions.timeoutMs ?? 300000, jsonrpc: true, args: [...(processOptions.args ?? []), ...ACP_HOSTS[host]] });
  let controls;
  try {
    const init = await client.request('initialize', { protocolVersion: 1, clientInfo: { name: 'craft_capabilities', version: runtimeVersion() },
      clientCapabilities: { fs: { readTextFile: false, writeTextFile: false }, terminal: false, session: { compaction: {} } } });
    if (init.protocolVersion !== 1) throw new Error('unsupported ACP protocol version');
    if (authenticate !== undefined) {
      if (!text(authenticate) || !init.authMethods?.some(method => method.id === authenticate)) throw new Error('ACP authentication method was not advertised');
      await client.request('authenticate', { methodId: authenticate });
    }
    if (init.agentCapabilities?.loadSession !== true && !init.agentCapabilities?.sessionCapabilities?.resume) throw new Error('ACP coordinator resume/load unsupported; use an already owned initialized client');
    controls = createAcpControls({ ...options, host, client, protocolVersion: init.protocolVersion, agentCapabilities: init.agentCapabilities });
    await controls.restoreCoordinator();
    return { ...controls, async close() { try { controls.close(); } finally { await client.close(); } } };
  } catch (error) { try { controls?.close(); } finally { await client.close(); } throw error; }
}
