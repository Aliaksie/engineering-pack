const hosts = ['claude','codex','cursor','copilot','gemini','agents','jetbrains','gemini-extension'];
const counts = ['guidanceFiles','guidanceBytes','estimatedTokens','evidenceCount','batchSize','cacheHits','cacheMisses','attemptNumber','selectedSkills','checkpoints','unavailableCapabilities'];
const enums = {
  phase: ['prepared','validated','publication-planned'],
  intent: ['implement','review','plan','verify','inspect','deliver','ship','fix','test','architecture','investigate','locate','status','run-check','configure-service','maintain-context'],
  decision: ['allow','deny','continue','compact','checkpoint','new','start-new-session','escalate','defer','reuse','fresh','unknown','unavailable','checkpoint-and-reset','continue-and-reassess','checkpoint-then-compact','save-handoff','retry-differently','run-check','design-verification','review','summarize-evidence','stop-blocked','return-control','escalate-model','recommend','stop','completed','inspect-failure','resolve-blocker','continue-bounded-step'],
  pressure: ['unknown','low','normal','moderate','high','critical','elevated','saturated','healthy'],
  capabilityClass: ['economical','standard','strong'],
  identityScope: ['session','invocation'],
  reason: ['observed','unavailable','stale','busy','invalid','disabled','budget-exhausted','deferred','recorded'],
  lifecycle: ['session-start','before-prompt','before-tool','after-tool','before-compact','after-compact','turn-end']
};
export const eventNames = ['session.observed','routing.completed','guidance.selected','discovery.completed','cache.observed','context.observed','session.advice','model.advice','attempt.recorded','checkpoint.saved','checkpoint.recovered','review.completed','host.observed'];

export function telemetryEvent({ name, timestamp, sessionId, operationId, packVersion, host, attributes = {}, outcome, durationMs }, identifiers = []) {
  if (!eventNames.includes(name) || typeof timestamp !== 'string' || !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/.test(timestamp)
    || !Number.isFinite(Date.parse(timestamp)) || !/^[a-f0-9]{32}$/.test(sessionId)
    || !/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(operationId)
    || /^0{8}-/.test(operationId) || !/^\d+\.\d+\.\d+$/.test(packVersion)) throw new Error('invalid telemetry envelope');
  const safe = {};
  for (const [key, value] of Object.entries(attributes).slice(0,64)) {
    if (counts.includes(key) && Number.isSafeInteger(value) && value >= 0 && value <= 1e9) safe[key] = value;
    else if (enums[key]?.includes(value)) safe[key] = value;
    else if (['reused','occupancyKnown'].includes(key) && typeof value === 'boolean') safe[key] = value;
    else if (key === 'skill' && identifiers.includes(value)) safe[key] = value;
  }
  return { schemaVersion: 1, name, timestamp, sessionId, operationId, packVersion,
    ...(hosts.includes(host) ? { host } : {}), attributes: safe,
    ...(['success','failure','deferred'].includes(outcome) ? { outcome } : {}),
    ...(Number.isFinite(durationMs) && durationMs >= 0 && durationMs <= 86400000 ? { durationMs } : {}) };
}
