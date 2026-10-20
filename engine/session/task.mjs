import { activityIdentity, appendAttempt, attemptReceipt, unresolvedAttempts } from './activity.mjs';
import { validateMetadata } from '../checkpoint.mjs';
import { relativeFile } from '../contracts/project.mjs';
import { digest } from '../cache/identity.mjs';

export const taskIntents = { plan: 'plan', implement: 'implement', verify: 'verify', review: 'review', locate: 'locate', explore: 'verify', architecture: 'investigate', story: 'story' };
const text = value => typeof value === 'string' && value.length > 0 && value.length <= 512 && !/[\x00-\x1f\x7f]/.test(value);
function stage(value) { if (!Object.hasOwn(taskIntents, value)) throw new Error('invalid task stage'); return value; }
function keys(value, allowed, label) {
  if (!value || typeof value !== 'object' || Array.isArray(value) || Object.keys(value).some(key => !allowed.includes(key))) throw new Error(`invalid ${label}`);
}
export function taskBinding(value) {
  if (value === null) return null;
  keys(value, ['formatVersion', 'task'], 'task binding');
  if (value.formatVersion !== 1) throw new Error('invalid task binding version');
  activityIdentity('binding', value.task);
  return value;
}
export function taskSource(repository) {
  return repository?.gitAvailable && repository.fingerprints?.complete
    ? digest([repository.head, repository.fingerprints.index, repository.fingerprints.workingTree]) : null;
}
export function startTask(state, input, repository) {
  keys(input, ['id', 'goal', 'stage', 'files'], 'task start');
  activityIdentity('task', input.id);
  if (!text(input.goal)) throw new Error('task goal must be 1..512 characters without control characters');
  if (state.taskRecord && (state.taskRecord.id !== input.id || state.taskRecord.goal !== input.goal)) throw new Error('task identity already has a different goal; use a new task id');
  const selected = stage(input.stage ?? state.taskRecord?.stage ?? 'implement');
  const files = input.files ?? state.taskRecord?.files ?? [];
  if (!Array.isArray(files) || files.length > 32) throw new Error('task files must be a bounded array');
  files.forEach(relativeFile);
  state.taskRecord ??= { id: input.id, goal: input.goal, status: 'working', goalComplete: false };
  if (state.taskRecord.stage && state.taskRecord.stage !== selected) {
    Object.assign(state.taskRecord, { status: 'working', goalComplete: false });
    delete state.taskRecord.verification; delete state.taskRecord.verificationRevision;
  }
  Object.assign(state.taskRecord, { stage: selected, files });
  return { state, intent: taskIntents[selected], context: taskContext(state, repository) };
}
export function taskContext(state, repository) {
  const record = state.taskRecord;
  if (!record) return { task: {}, stale: false };
  if (!text(record.id) || !text(record.goal)) throw new Error('invalid saved task record');
  stage(record.stage);
  const source = taskSource(repository);
  const stale = Boolean(record.summary && (!source || record.source !== source));
  const task = Object.fromEntries(['id', 'stage', 'status', 'goalComplete', 'files', 'revision', 'verification', 'verificationRevision', 'safeBoundary', 'nextStep'].filter(key => record[key] !== undefined && record[key] !== null).map(key => [key, record[key]]));
  if (stale) Object.assign(task, { status: 'working', goalComplete: false, verification: 'unknown', verificationRevision: undefined });
  return { task, stale, summary: record.summary ?? null, nextStep: record.nextStep ?? null };
}
export function recordTaskStep(state, input, repository, id, at) {
  keys(input, ['stage', 'target', 'approach', 'evidence', 'outcome', 'result', 'nextStep', 'complete', 'profile', 'maxAttempts'], 'task step');
  const record = state.taskRecord;
  if (!record) throw new Error('task-step requires task-start for this session and task');
  stage(input.stage ?? record.stage);
  for (const field of ['result', 'nextStep']) if (!text(input[field])) throw new Error(`task step ${field} must be 1..512 characters without control characters`);
  if (input.complete !== undefined && typeof input.complete !== 'boolean') throw new Error('task completion must be boolean');
  if (input.complete && input.outcome !== 'passed') throw new Error('task completion requires a passed acceptance receipt');
  const selected = input.stage ?? record.stage;
  const receipt = attemptReceipt({ operation: selected, target: input.target, approach: input.approach, evidence: input.evidence, outcome: input.outcome,
    ...(input.profile !== undefined ? { profile: input.profile } : {}), ...(input.maxAttempts !== undefined ? { maxAttempts: input.maxAttempts } : {}) }, repository, id, at);
  appendAttempt(state, receipt);
  if (input.complete && unresolvedAttempts(state.attempts).length) throw new Error('task completion has unresolved acceptance failures or blockers');
  Object.assign(record, { stage: selected, status: input.outcome === 'passed' ? 'completed' : input.outcome === 'quality-failure' ? 'failed' : 'blocked',
    goalComplete: input.complete === true, revision: repository.head, source: receipt.source, safeBoundary: true,
    summary: input.result, nextStep: input.nextStep });
  if (selected === 'verify') Object.assign(record, { verification: input.outcome === 'passed' ? 'passed' : 'failed', verificationRevision: repository.head });
  else { delete record.verification; delete record.verificationRevision; }
  if (input.outcome === 'passed' && !record.completed?.endsWith(input.result)) record.completed = [record.completed, input.result].filter(Boolean).join('\n').slice(-1024);
  const continuation = { goal: record.goal, currentState: input.result,
    completed: record.completed ?? '', approach: input.approach, evidence: input.evidence,
    filesChanged: record.files.join(', ').slice(0, 1024), validation: `${input.outcome}: ${input.evidence}`,
    remainingWork: input.complete ? '' : input.nextStep, blockers: record.status === 'blocked' ? input.result : '', nextStep: input.nextStep };
  validateMetadata({ taskId: record.id, continuation });
  return { state, receipt, continuation };
}
