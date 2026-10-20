import { route } from '../routing/route.mjs';
import { digest } from '../cache/identity.mjs';
const object = (value) => value !== null && typeof value === 'object' && !Array.isArray(value);
const count = (value) => Number.isSafeInteger(value) && value >= 0;
const text = (value) => typeof value === 'string' && value.length > 0 && value.length <= 512 && !/[\x00-\x1f\x7f]/.test(value);
function keys(value, allowed, label) {
  if (!object(value) || Object.keys(value).some((key) => !allowed.includes(key))) throw new Error(`invalid ${label} fields`);
}
// Pure recommendations. Callers provide explicit task state and observations; no transcript
// parsing, tools, state writes or model calls are needed to choose the next useful action.
export function sessionAdvice(input = {}, project = { formatVersion: 1 }, { now, catalog } = {}) {
  keys(input, ['sessionId', 'task', 'usage', 'policy', 'previous', 'host', 'activity', 'packContext'], 'session input');
  if (input.sessionId !== undefined && !text(input.sessionId)) throw new Error('invalid sessionId');
  const context = project.efficiency?.context ?? {};
  const policy = { elevatedAt: context.elevatedAt ?? 0.6, compactAt: context.compactAt ?? 0.75, restartAt: context.restartAt ?? 0.85, maxAgeMs: 300000, cooldownMs: 300000, ...input.policy };
  if (input.policy !== undefined) keys(input.policy, ['elevatedAt', 'compactAt', 'restartAt', 'maxAgeMs', 'cooldownMs'], 'session policy');
  if (!Number.isFinite(policy.elevatedAt) || !Number.isFinite(policy.compactAt) || !Number.isFinite(policy.restartAt) || !(policy.elevatedAt > 0 && policy.elevatedAt < policy.compactAt && policy.compactAt < policy.restartAt && policy.restartAt <= 1)
    || !count(policy.maxAgeMs) || !count(policy.cooldownMs) || !Number.isFinite(now)) throw new Error('invalid session thresholds');
  const task = input.task ?? {};
  keys(task, ['id', 'revision', 'verificationRevision', 'verificationMode', 'stage', 'status', 'verification', 'goalComplete', 'reviewRequested', 'files', 'handoffSaved', 'safeBoundary', 'phaseChanged', 'staleAssumptions', 'compacted', 'almostComplete', 'checkpointFresh', 'risks', 'operation', 'nextStep'], 'task');
  for (const field of ['id', 'revision', 'verificationRevision', 'operation', 'nextStep']) if (task[field] !== undefined && !text(task[field])) throw new Error(`invalid task ${field}`);
  if (task.stage !== undefined && !['plan', 'implement', 'verify', 'review', 'explore', 'story', 'architecture', 'locate', 'discover'].includes(task.stage)) throw new Error('invalid task stage');
  if (task.verificationMode !== undefined && (task.stage !== 'verify' || !['manual', 'automation', 'test-code'].includes(task.verificationMode))) throw new Error('invalid task verificationMode');
  if (task.status !== undefined && !['working', 'completed', 'failed', 'blocked', 'unknown'].includes(task.status)) throw new Error('invalid task status');
  if (task.verification !== undefined && !['passed', 'failed', 'pending', 'unknown'].includes(task.verification)) throw new Error('invalid verification');
  for (const field of ['goalComplete', 'reviewRequested', 'handoffSaved', 'safeBoundary', 'phaseChanged', 'staleAssumptions', 'compacted', 'almostComplete', 'checkpointFresh']) if (task[field] !== undefined && typeof task[field] !== 'boolean') throw new Error(`invalid task ${field}`);
  if (task.risks !== undefined && (!Array.isArray(task.risks) || task.risks.length > 16 || task.risks.some(risk => !text(risk)))) throw new Error('invalid task risks');
  if (task.files !== undefined && (!Array.isArray(task.files) || task.files.length > 100 || task.files.some((file) => typeof file !== 'string'))) throw new Error('invalid task files');
  const plan = route({ intent: 'run-check', files: task.files ?? [], project, catalog });
  let usage = { status: 'unknown', confidence: 'unknown', reason: 'No current-context observation supplied.', currentTokens: null, capacityTokens: null, usedRatio: null };
  if (input.usage !== undefined) {
    keys(input.usage, ['kind', 'currentTokens', 'capacityTokens', 'observedAt', 'source', 'sessionId'], 'usage');
    const observed = input.usage;
    const at = Date.parse(observed.observedAt);
    if (observed.kind !== 'current-context') usage.reason = 'Cumulative or unspecified token counts do not establish current context occupancy.';
    else if (!count(observed.currentTokens) || !count(observed.capacityTokens) || observed.capacityTokens === 0 || observed.currentTokens > observed.capacityTokens) usage.reason = 'Current-context counts are invalid or incomplete.';
    else if (!text(observed.source) || !Number.isFinite(at) || at > now || now - at > policy.maxAgeMs) usage.reason = 'Observation source or freshness is unavailable, stale or invalid.';
    else if (!input.sessionId || observed.sessionId !== input.sessionId) usage.reason = 'Observation does not identify this session.';
    else usage = { status: 'observed', confidence: 'observed', source: observed.source, observedAt: observed.observedAt,
      currentTokens: observed.currentTokens, capacityTokens: observed.capacityTokens, usedRatio: observed.currentTokens / observed.capacityTokens };
  }
  const host = input.host ?? {};
  keys(host, ['name', 'model', 'reliableCompaction'], 'host');
  for (const field of ['name', 'model']) if (host[field] !== undefined && !text(host[field])) throw new Error(`invalid host ${field}`);
  if (host.reliableCompaction !== undefined && typeof host.reliableCompaction !== 'boolean') throw new Error('invalid host reliableCompaction');
  const activity = input.activity ?? {};
  const activityFields = ['discoveryRequests', 'repeatedLookups', 'batchedOperations', 'reuseHits', 'checkpoints', 'attempts', 'maxAttempts', 'retries', 'modelEscalations', 'noProgressAttempts', 'freshLookups', 'routingDecisions', 'filesRequested', 'referencesRequested', 'operationsAttempted'];
  keys(activity, activityFields, 'activity');
  for (const [field, value] of Object.entries(activity)) if (!count(value)) throw new Error(`invalid activity ${field}`);
  if (activity.maxAttempts === 0) throw new Error('maxAttempts must be positive');
  if (input.packContext !== undefined) {
    keys(input.packContext, ['estimatedTokens', 'skills', 'references'], 'pack context');
    for (const [field, value] of Object.entries(input.packContext)) if (!count(value)) throw new Error(`invalid pack context ${field}`);
  }
  const pressure = usage.status !== 'observed' ? 'unknown' : usage.usedRatio >= policy.restartAt ? 'saturated'
    : usage.usedRatio >= policy.compactAt ? 'high' : usage.usedRatio >= policy.elevatedAt ? 'elevated' : 'healthy';
  const exhausted = activity.maxAttempts !== undefined && (activity.attempts ?? 0) >= activity.maxAttempts;
  const repeatedDiscovery = Math.max(0, (activity.repeatedLookups ?? 0) - (activity.reuseHits ?? 0));
  const looping = (activity.noProgressAttempts ?? 0) >= 3;
  const boundary = task.safeBoundary === true || task.status === 'completed';
  const checkpointReady = task.handoffSaved === true && task.checkpointFresh === true;
  let suggestion = null;
  const suggest = (action, reason, effect, prerequisites = [], extra = {}) => ({ action, reason, effect, prerequisites, ...extra });
  const reset = () => checkpointReady
    ? suggest('start-new-session', 'A fresh continuation checkpoint is declared saved; resume from it and current repository evidence.', 'session-reset', ['Read checkpoint, inspect current diff and route only necessary guidance.'])
    : suggest('save-handoff', 'Preserve a small continuation checkpoint before replacing session context.', 'local-state-write', ['Save goal, state, completed work, approach, evidence identifiers, changed paths, validation, remaining work, blockers and next step.'], { afterCheckpoint: 'start-new-session' });
  if (exhausted || looping) {
    suggestion = suggest('return-control', 'The declared attempt budget is exhausted or three attempts made no progress.', 'read', ['Preserve failure evidence. Use model-advice for evidence-bound escalation; do not repeat the same attempt.']);
  } else if (task.status === 'failed' || task.verification === 'failed') {
    suggestion = suggest('inspect-failure', 'Preserve and diagnose the observed failure before rerunning or advancing.', 'read');
  } else if (task.status === 'blocked') {
    suggestion = suggest('resolve-blocker', 'A recorded prerequisite is missing; identify it without inventing completion.', 'read');
  } else if (task.goalComplete === true) {
    suggestion = suggest('summarize-evidence', 'The task is declared complete; return the outcome and evidence.', 'read');
  } else if (['high', 'saturated'].includes(pressure)) {
    if (!boundary || task.almostComplete === true) {
      suggestion = suggest('continue-bounded-step', 'Finish the useful bounded step before changing context at a safe boundary.', 'read', [], { atBoundary: pressure === 'saturated' ? 'checkpoint-then-reconsider-context' : 'reconsider-compaction' });
    } else if (pressure === 'saturated' && (task.phaseChanged || task.staleAssumptions || (task.compacted && repeatedDiscovery >= 3))) {
      suggestion = reset();
    } else if (host.reliableCompaction === true) {
      suggestion = checkpointReady
        ? suggest('compact', 'Context pressure is high and a safe task boundary is available.', 'session-context-change', ['Use the host compaction control; retain the continuation checkpoint.'])
        : suggest('save-handoff', 'Save continuation evidence at this boundary before compaction.', 'local-state-write', [], { afterCheckpoint: 'compact' });
    } else {
      suggestion = reset();
    }
  } else if ((activity.noProgressAttempts ?? 0) > 0) {
    suggestion = suggest('retry-differently', 'A no-progress attempt is recorded; inspect evidence and change the approach before retrying.', 'read');
  } else if (task.status === 'completed' && task.goalComplete !== true) {
    const verified = task.verification === 'passed' && task.revision && task.verificationRevision === task.revision;
    if (['implement', 'verify'].includes(task.stage) && !verified) {
      suggestion = task.verificationMode !== 'manual' && plan.checks.length
        ? suggest('run-check', 'Implementation is recorded complete; affected declared checks still need evidence.', 'command-execution', ['Original task authorization and current command/environment policy still apply.'], { checks: plan.checks })
        : suggest('design-verification', 'Verification for the current target still needs an appropriate check or observation.', 'read', [], { skill: 'craft-engineering:workflow-verify-feature' });
    } else if (['implement', 'verify'].includes(task.stage) && verified && task.reviewRequested === true) {
      suggestion = suggest('review', 'Verification is recorded passed and review was explicitly requested.', 'read', ['Review the same revision covered by the evidence.'], { skill: 'craft-review:review-change' });
    } else if (task.nextStep) {
      suggestion = suggest('continue-task', task.nextStep, 'recommendation-only', ['Continue only within existing task scope and authorization.']);
    } else if (task.stage !== 'plan') {
      suggestion = suggest('summarize-evidence', 'Return the completed outcome, acceptance evidence and remaining limits.', 'read');
    }
  }
  let previous = null;
  if (input.previous !== undefined) {
    keys(input.previous, ['key', 'at'], 'previous suggestion');
    if (!text(input.previous.key) || !Number.isFinite(Date.parse(input.previous.at))) throw new Error('invalid previous suggestion');
    previous = input.previous;
  }
  if (project.efficiency?.advice?.enabled === false || project.efficiency?.advice?.nextStep === false) suggestion = null;
  if (project.efficiency?.context?.action === 'manual' && ['compact', 'start-new-session', 'save-handoff'].includes(suggestion?.action)) suggestion = null;
  const key = suggestion ? digest({ session: input.sessionId, task, suggestion }) : null;
  const suppressed = Boolean(key && previous?.key === key && now >= Date.parse(previous.at) && now - Date.parse(previous.at) < policy.cooldownMs);
  return { formatVersion: 1, mode: 'recommend', usage, pressure, operation: task.operation ?? task.stage ?? 'unknown', host: { name: host.name ?? 'unknown', model: host.model ?? 'unknown', reliableCompaction: host.reliableCompaction ?? null },
    packContext: { scope: 'caller-declared selected guidance; estimated tokens, not occupancy', estimatedTokens: null, skills: null, references: null, ...(input.packContext ?? {}) },
    activity: { scope: 'caller-declared Engineering Pack activity; not provider usage', ...Object.fromEntries(activityFields.map(field => [field, null])), ...activity }, recommendation: suggestion, suggestion: suppressed ? null : suggestion, suppressed,
    notice: key ? suppressed ? previous : { key, at: new Date(now).toISOString() } : null,
    authorization: 'advice-only; no command, compaction, reset or checkpoint write was executed',
    evidence: 'Task completion and verification are caller declarations, not inferred or independently certified.' };
}

