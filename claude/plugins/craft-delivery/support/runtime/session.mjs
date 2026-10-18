import fs from 'node:fs';
import { createHash } from 'node:crypto';
import { route } from './router.mjs';

const object = (value) => value !== null && typeof value === 'object' && !Array.isArray(value);
const count = (value) => Number.isSafeInteger(value) && value >= 0;
const text = (value) => typeof value === 'string' && value.length > 0 && value.length <= 512 && !/[\x00-\x1f\x7f]/.test(value);
function keys(value, allowed, label) {
  if (!object(value) || Object.keys(value).some((key) => !allowed.includes(key))) throw new Error(`invalid ${label} fields`);
}
export function readSessionInput(file) {
  if (fs.lstatSync(file).isSymbolicLink() || !fs.statSync(file).isFile() || fs.statSync(file).size > 65536) throw new Error('session input must be a bounded regular JSON file');
  return JSON.parse(fs.readFileSync(file, 'utf8'));
}

// Pure recommendations. Callers provide explicit task state and observations; no transcript
// parsing, tools, state writes or model calls are needed to choose the next useful action.
export function sessionAdvice(input = {}, project = { formatVersion: 1 }, { now = Date.now() } = {}) {
  keys(input, ['sessionId', 'task', 'usage', 'policy', 'previous'], 'session input');
  if (input.sessionId !== undefined && !text(input.sessionId)) throw new Error('invalid sessionId');
  const policy = { compactAt: 0.8, restartAt: 0.95, maxAgeMs: 300000, cooldownMs: 300000, ...input.policy };
  if (input.policy !== undefined) keys(input.policy, ['compactAt', 'restartAt', 'maxAgeMs', 'cooldownMs'], 'session policy');
  if (!Number.isFinite(policy.compactAt) || !Number.isFinite(policy.restartAt) || !(policy.compactAt > 0 && policy.compactAt < policy.restartAt && policy.restartAt <= 1)
    || !count(policy.maxAgeMs) || !count(policy.cooldownMs) || !Number.isFinite(now)) throw new Error('invalid session thresholds');
  const task = input.task ?? {};
  keys(task, ['id', 'revision', 'verificationRevision', 'verificationMode', 'stage', 'status', 'verification', 'goalComplete', 'reviewRequested', 'files', 'handoffSaved'], 'task');
  for (const field of ['id', 'revision', 'verificationRevision']) if (task[field] !== undefined && !text(task[field])) throw new Error(`invalid task ${field}`);
  if (task.stage !== undefined && !['plan', 'implement', 'verify', 'review', 'explore', 'story', 'architecture'].includes(task.stage)) throw new Error('invalid task stage');
  if (task.verificationMode !== undefined && (task.stage !== 'verify' || !['manual', 'automation', 'test-code'].includes(task.verificationMode))) throw new Error('invalid task verificationMode');
  if (task.status !== undefined && !['working', 'completed', 'failed', 'blocked', 'unknown'].includes(task.status)) throw new Error('invalid task status');
  if (task.verification !== undefined && !['passed', 'failed', 'pending', 'unknown'].includes(task.verification)) throw new Error('invalid verification');
  for (const field of ['goalComplete', 'reviewRequested', 'handoffSaved']) if (task[field] !== undefined && typeof task[field] !== 'boolean') throw new Error(`invalid task ${field}`);
  if (task.files !== undefined && (!Array.isArray(task.files) || task.files.length > 100 || task.files.some((file) => typeof file !== 'string'))) throw new Error('invalid task files');
  const plan = route({ intent: 'run-check', files: task.files ?? [], project });
  let usage = { status: 'unknown', reason: 'No current-context observation supplied.', currentTokens: null, capacityTokens: null, usedRatio: null };
  if (input.usage !== undefined) {
    keys(input.usage, ['kind', 'currentTokens', 'capacityTokens', 'observedAt', 'source', 'sessionId'], 'usage');
    const observed = input.usage;
    const at = Date.parse(observed.observedAt);
    if (observed.kind !== 'current-context') usage.reason = 'Cumulative or unspecified token counts do not establish current context occupancy.';
    else if (!count(observed.currentTokens) || !count(observed.capacityTokens) || observed.capacityTokens === 0 || observed.currentTokens > observed.capacityTokens) usage.reason = 'Current-context counts are invalid or incomplete.';
    else if (!text(observed.source) || !Number.isFinite(at) || at > now || now - at > policy.maxAgeMs) usage.reason = 'Observation source or freshness is unavailable, stale or invalid.';
    else if (!input.sessionId || observed.sessionId !== input.sessionId) usage.reason = 'Observation does not identify this session.';
    else usage = { status: 'observed', source: observed.source, observedAt: observed.observedAt,
      currentTokens: observed.currentTokens, capacityTokens: observed.capacityTokens, usedRatio: observed.currentTokens / observed.capacityTokens };
  }
  let suggestion = null;
  const suggest = (action, reason, effect, prerequisites = [], extra = {}) => ({ action, reason, effect, prerequisites, ...extra });
  if (usage.status === 'observed' && usage.usedRatio >= policy.restartAt) {
    suggestion = task.handoffSaved === true
      ? suggest('start-new-session', 'Observed context exceeds the configured restart threshold; an explicit handoff is saved.', 'session-reset', ['Recheck saved scope and evidence in the new session.'])
      : suggest('save-handoff', 'Observed context exceeds the configured restart threshold.', 'local-state-write', ['Save goal, scope, decisions, revision, acceptance evidence, limits and next step before starting a new session.']);
  } else if (usage.status === 'observed' && usage.usedRatio >= policy.compactAt) {
    suggestion = suggest('compact', 'Observed context exceeds the configured compaction threshold.', 'session-context-change', ['Preserve current task and evidence before using the host compaction control.']);
  } else if (task.status === 'failed' || task.verification === 'failed') {
    suggestion = suggest('inspect-failure', 'Preserve and diagnose the observed failure before rerunning or advancing.', 'read');
  } else if (task.status === 'blocked') {
    suggestion = suggest('resolve-blocker', 'A recorded prerequisite is missing; identify it without inventing completion.', 'read');
  } else if (task.status === 'completed' && task.goalComplete !== true) {
    const verified = task.verification === 'passed' && task.revision && task.verificationRevision === task.revision;
    if (['implement', 'verify'].includes(task.stage) && !verified) {
      suggestion = task.verificationMode !== 'manual' && plan.checks.length
        ? suggest('run-check', 'Implementation is recorded complete; affected declared checks still need evidence.', 'command-execution', ['Original task authorization and current command/environment policy still apply.'], { checks: plan.checks })
        : suggest('design-verification', 'Verification for the current target still needs an appropriate check or observation.', 'read', [], { skill: 'craft-engineering:workflow-verify-feature' });
    } else if (['implement', 'verify'].includes(task.stage) && verified && task.reviewRequested === true) {
      suggestion = suggest('review', 'Verification is recorded passed and review was explicitly requested.', 'read', ['Review the same revision covered by the evidence.'], { skill: 'craft-review:review-change' });
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
  const key = suggestion ? createHash('sha256').update(JSON.stringify({ session: input.sessionId, task, suggestion })).digest('hex') : null;
  const suppressed = Boolean(key && previous?.key === key && now >= Date.parse(previous.at) && now - Date.parse(previous.at) < policy.cooldownMs);
  return { formatVersion: 1, usage, suggestion: suppressed ? null : suggestion, suppressed,
    notice: key ? suppressed ? previous : { key, at: new Date(now).toISOString() } : null,
    authorization: 'advice-only; no command, compaction, reset or state write was executed',
    evidence: 'Task completion and verification are caller declarations, not inferred or independently certified.' };
}
