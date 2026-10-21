const hosts = ['claude','codex','cursor','copilot','gemini','agents','jetbrains','gemini-extension'];
const counts = ['scratchBytes','evidenceBytes','workspacesCleaned','guidanceFiles','guidanceBytes','estimatedTokens','evidenceCount','batchSize','cacheHits','cacheMisses','recordedAttempts','selectedSkills','checkpoints','unavailableCapabilities','droppedEvents','exportedEvents','rejectedEvents','statusCode'];
const enums = {
  workspaceAction: ['open','write','promote','release','complete','maintain','touch'],
  package: ['craft-delivery','craft-engineering','craft-review','craft-stories'],
  inputStatus: ['complete','invalid','oversized','read-failure'],
  area: ['cli','host','project','routing','guidance','discovery','cache','context','session','model','attempt','checkpoint','review','integration','state','telemetry'],
  command: ['workspace-open','workspace-write','workspace-promote','workspace-release','workspace-clean','worker-status','task-feature','task-features','telemetry-capture','telemetry-forget','telemetry-report','task-offer','task-decision','task-action-check','settings','task-start','task-step','route','context-report','session','session-advice','session-attempt','model-advice','lookup','locate','prepare-services','operation','checkpoint-save','checkpoint-read','unknown'],
  operation: ['git.status','git.diff','git.commit','git.push','change.read','change.diff','change.publish','task.read','task.search','task.transition','task.comment'],
  phase: ['prepared','validated','publication-planned'],
  intent: ['implement','review','plan','verify','inspect','deliver','ship','fix','test','architecture','investigate','locate','status','run-check','configure-service','maintain-context'],
  decision: ['continue-task','allow','deny','continue','compact','checkpoint','new','start-new-session','escalate','defer','reuse','fresh','unknown','unavailable','checkpoint-and-reset','continue-and-reassess','checkpoint-then-compact','save-handoff','retry-differently','run-check','design-verification','review','summarize-evidence','stop-blocked','return-control','escalate-model','recommend','stop','completed','inspect-failure','resolve-blocker','continue-bounded-step'],
  pressure: ['unknown','low','normal','moderate','high','critical','elevated','saturated','healthy'],
  capabilityClass: ['economical','standard','strong'],
  identityScope: ['session','invocation'],
  reason: ['observed','unavailable','stale','busy','invalid','disabled','budget-exhausted','deferred','recorded','missing','incomplete'],
  lifecycle: ['session-start','before-prompt','before-tool','after-tool','before-compact','after-compact','turn-end']
};
export const eventNames = ['workspace.observed','worker.dispatch','cli.command','host.hook','host.session','project.loaded','input.loaded','search.completed','cache.lookup','cache.invalidated','state.read','state.updated','checkpoint.read','review.command','review.publication-planned','integration.prepared','integration.planned','integration.observed','integration.inbox','telemetry.export','diagnostic','session.observed','routing.completed','guidance.selected','discovery.completed','cache.observed','context.observed','session.advice','model.advice','attempt.recorded','checkpoint.saved','checkpoint.recovered','review.completed','host.observed'];

// This JSON contract has no runtime, host SDK, or provider dependencies. Legacy
// schema-1 events without trace fields remain readable; new producers supply them.
export const reasonCodes = ['OPERATION_FAILED','INVALID_INPUT','INPUT_TOO_LARGE','CONFIG_INVALID','ACCESS_DENIED','NOT_FOUND',
  'STATE_BUSY','STATE_UNAVAILABLE','STORAGE_FULL','CACHE_UNAVAILABLE','CHECKPOINT_UNAVAILABLE','CHECKPOINT_STALE',
  'CONTEXT_UNAVAILABLE','HOST_CAPABILITY_UNAVAILABLE','HOST_OBSERVATION_UNAVAILABLE','HOOK_FAILED','SEARCH_FAILED','SEARCH_INCOMPLETE',
  'ROUTING_FAILED','REVIEW_INVALID','VALIDATION_FAILED','INTEGRATION_UNAVAILABLE','ATTEMPT_BUDGET_EXHAUSTED',
  'MODEL_ADVICE_UNAVAILABLE','TELEMETRY_CONFIG_INVALID','TELEMETRY_EVENT_INVALID','TELEMETRY_METADATA_UNAVAILABLE','TELEMETRY_IDENTITY_UNAVAILABLE','TELEMETRY_STORAGE_UNAVAILABLE','TELEMETRY_EVENTS_DROPPED',
  'EXPORT_QUEUE_FULL','EXPORT_FAILED','EXPORT_TIMEOUT','EXPORT_PARTIAL_REJECTED','EXPORT_WARNING','EXPORT_INVALID_RESPONSE'];
const operationIdPattern = /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/;
const validOperation = value => typeof value === 'string' && operationIdPattern.test(value)
  && value.replaceAll('-','').slice(0,16) !== '0'.repeat(16);
export function traceContext({ traceId, operationId, parentOperationId }) {
  if (typeof traceId !== 'string' || !/^[a-f0-9]{32}$/.test(traceId) || traceId === '0'.repeat(32)
    || !validOperation(operationId) || (parentOperationId !== undefined && (!validOperation(parentOperationId) || parentOperationId === operationId))) throw new Error('invalid trace context');
  return { traceId, operationId, ...(parentOperationId ? { parentOperationId } : {}) };
}
export function telemetryEvent({ name, timestamp, sessionId, operationId, traceId, parentOperationId, packVersion, host,
  attributes = {}, outcome, durationMs, reasonCode, kind }, identifiers = []) {
  if (!eventNames.includes(name) || typeof timestamp !== 'string' || !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/.test(timestamp)
    || !Number.isFinite(Date.parse(timestamp)) || Date.parse(timestamp) < 0 || !/^[a-f0-9]{32}$/.test(sessionId)
    || (packVersion !== null && (typeof packVersion !== 'string' || !/^\d+\.\d+\.\d+$/.test(packVersion)))) throw new Error('invalid telemetry envelope');
  const context = traceContext({ traceId: traceId ?? operationId?.replaceAll('-',''), operationId, parentOperationId });
  if (name === 'diagnostic' && !reasonCodes.includes(reasonCode)) throw new Error('diagnostic requires a stable reason code');
  const safe = {};
  for (const [key, value] of Object.entries(attributes).slice(0,64)) {
    if (counts.includes(key) && Number.isSafeInteger(value) && value >= 0 && value <= 1e9) safe[key] = value;
    else if (Object.hasOwn(enums,key) && enums[key].includes(value)) safe[key] = value;
    else if (['reused','occupancyKnown','available','complete','contextSupported'].includes(key) && typeof value === 'boolean') safe[key] = value;
    else if (key === 'skill' && identifiers.includes(value)) safe[key] = value;
  }
  return { schemaVersion: 1, name, kind: name === 'diagnostic' ? 'diagnostic' : kind === 'operation' ? 'operation' : 'observation',
    timestamp, sessionId, ...context, packVersion,
    ...(hosts.includes(host) ? { host } : {}), attributes: safe,
    ...(reasonCodes.includes(reasonCode) ? { reasonCode } : {}),
    ...(['success','failure','deferred'].includes(outcome) ? { outcome } : {}),
    ...(Number.isFinite(durationMs) && durationMs >= 0 && durationMs <= 86400000 ? { durationMs } : {}) };
}
