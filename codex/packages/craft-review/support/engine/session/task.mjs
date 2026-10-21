import { activityIdentity, appendAttempt, attemptReceipt, unresolvedAttempts } from './activity.mjs';
import { validateMetadata } from '../checkpoint.mjs';
import { relativeFile } from '../contracts/project.mjs';
import { digest } from '../cache/identity.mjs';
import { repositoryScopes } from '../repositories.mjs';
import { privatePath } from '../contracts/paths.mjs';

export const taskIntents = { plan: 'plan', implement: 'implement', verify: 'verify', review: 'review', locate: 'locate', explore: 'verify', architecture: 'investigate', story: 'story' };
const text = value => typeof value === 'string' && value.length > 0 && value.length <= 512 && !/[\x00-\x1f\x7f]/.test(value);
function stage(value) { if (!Object.hasOwn(taskIntents, value)) throw new Error('invalid task stage'); return value; }
function keys(value, allowed, label) {
  if (!value || typeof value !== 'object' || Array.isArray(value) || Object.keys(value).some(key => !allowed.includes(key))) throw new Error(`invalid ${label}`);
}
export function taskBinding(value, repository) {
  if (value === null) return null;
  keys(value, ['formatVersion', 'task', 'checkout', 'repositories'], 'task binding');
  if (value.formatVersion !== 1) throw new Error('invalid task binding version');
  activityIdentity('binding', value.task);
  if (value.repositories !== undefined) repositoryScopes(value.repositories);
  if (value.checkout !== undefined && value.checkout !== null && (typeof value.checkout !== 'string' || value.checkout.length > 1024)) throw new Error('invalid task checkout binding');
  return repository && (value.checkout === undefined || value.checkout !== (repository.checkout ?? null)) ? null : value;
}
export function taskContract(record) {
  return record ? digest([record.id, record.goal, record.stage, record.files ?? [], record.risks ?? [], record.checkout ?? null, ...(record.repositories ? [record.repositories] : [])]) : null;
}
export function taskSource(repository) {
  return repository?.gitAvailable && repository.fingerprints?.complete
    ? digest([repository.head, repository.fingerprints.index, repository.fingerprints.workingTree]) : null;
}
export function startTask(state, input, repository) {
  keys(input, ['id', 'goal', 'stage', 'files', 'risks', 'reviewAuthorization', 'repositories'], 'task start');
  activityIdentity('task', input.id);
  if (!text(input.goal)) throw new Error('task goal must be 1..512 characters without control characters');
  if (state.taskRecord && (state.taskRecord.id !== input.id || state.taskRecord.goal !== input.goal)) throw new Error('task identity already has a different goal; use a new task id');
  const selected = stage(input.stage ?? state.taskRecord?.stage ?? 'implement');
  const files = input.files ?? state.taskRecord?.files ?? [];
  if (!Array.isArray(files) || files.length > 32) throw new Error('task files must be a bounded array');
  files.forEach(relativeFile);
  if (files.some(privatePath)) throw new Error('private workspace files are not task source');
  const repositories = input.repositories === undefined ? state.taskRecord?.repositories : repositoryScopes(input.repositories);
  if (repositories && files.some(file => !repositories.some(scope => scope === '.' || file === scope || file.startsWith(`${scope}/`)))) throw new Error('task files must belong to a selected repository');
  if (input.risks !== undefined && (!Array.isArray(input.risks) || input.risks.length > 16 || input.risks.some(value => !text(value)))) throw new Error('invalid task risks');
  if (input.reviewAuthorization !== undefined && !text(input.reviewAuthorization)) throw new Error('invalid review authorization evidence');
  state.taskRecord ??= { id: input.id, goal: input.goal, status: 'working', goalComplete: false };
  const risks = input.risks ?? state.taskRecord.risks ?? [];
  const changed = state.taskRecord.stage && (state.taskRecord.stage !== selected
    || JSON.stringify(state.taskRecord.files ?? []) !== JSON.stringify(files)
    || JSON.stringify(state.taskRecord.risks ?? []) !== JSON.stringify(risks)
    || JSON.stringify(state.taskRecord.repositories) !== JSON.stringify(repositories)
    || state.taskRecord.checkout !== (repository.checkout ?? null));
  if (changed) {
    Object.assign(state.taskRecord, { status: 'working', goalComplete: false });
    delete state.taskRecord.verification; delete state.taskRecord.verificationRevision;
    delete state.taskRecord.completed; state.taskRecord.safeBoundary = false;
  }
  Object.assign(state.taskRecord, { stage: selected, files, risks, checkout: repository.checkout ?? null, ...(input.reviewAuthorization ? { reviewAuthorization: input.reviewAuthorization, reviewRequested: true } : {}) });
  if (repositories !== undefined) state.taskRecord.repositories = repositories;
  return { state, intent: taskIntents[selected], context: taskContext(state, repository) };
}
export function taskContext(state, repository) {
  const record = state.taskRecord;
  if (!record) return { task: {}, stale: false };
  if (!text(record.id) || !text(record.goal)) throw new Error('invalid saved task record');
  stage(record.stage);
  const source = taskSource(repository);
  const stale = Boolean(record.summary && (!source || record.source !== source
    || record.acceptedContract !== taskContract(record) || record.checkout !== (repository.checkout ?? null)));
  const task = Object.fromEntries(['id', 'stage', 'status', 'goalComplete', 'files', 'revision', 'verification', 'verificationRevision', 'safeBoundary', 'nextStep', 'risks', 'reviewRequested'].filter(key => record[key] !== undefined && record[key] !== null).map(key => [key, record[key]]));
  if (stale) Object.assign(task, { status: 'working', goalComplete: false, verification: 'unknown', verificationRevision: undefined, safeBoundary: false });
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
  Object.assign(record, { stage: selected, status: input.outcome === 'passed' ? 'completed' : input.outcome === 'quality-failure' ? 'failed' : input.outcome === 'prerequisite-resolved' ? 'working' : 'blocked',
    goalComplete: input.complete === true, revision: repository.head, source: receipt.source, safeBoundary: true,
    summary: input.result, nextStep: input.nextStep });
  if (selected === 'verify' && input.outcome !== 'prerequisite-resolved') Object.assign(record, { verification: input.outcome === 'passed' ? 'passed' : 'failed', verificationRevision: repository.head });
  else { delete record.verification; delete record.verificationRevision; }
  record.acceptedContract = taskContract(record);
  if (input.outcome === 'passed' && !record.completed?.endsWith(input.result)) record.completed = [record.completed, input.result].filter(Boolean).join('\n').slice(-1024);
  const continuation = { goal: record.goal, currentState: input.result,
    completed: record.completed ?? '', approach: input.approach, evidence: input.evidence,
    filesChanged: record.files.join(', ').slice(0, 1024), validation: `${input.outcome}: ${input.evidence}`,
    remainingWork: input.complete ? '' : input.nextStep, blockers: record.status === 'blocked' ? input.result : '', nextStep: input.nextStep };
  validateMetadata({ taskId: record.id, continuation });
  return { state, receipt, continuation };
}
