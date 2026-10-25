import { validateAcceptanceEvidence, engineeringContractDigest, acceptanceCoverage } from './acceptance.mjs';
import { activityIdentity, appendAttempt, attemptReceipt, unresolvedAttempts } from './activity.mjs';
import { validateMetadata } from '../checkpoint.mjs';
import { digest } from '../cache/identity.mjs';
import { repositoryScopes } from '../repositories.mjs';

const stages = ['plan', 'implement', 'verify', 'review', 'locate', 'explore', 'architecture', 'story'];
const text = value => typeof value === 'string' && value.length > 0 && value.length <= 512 && !/[\x00-\x1f\x7f]/.test(value);
function stage(value) { if (!stages.includes(value)) throw new Error('invalid task stage'); return value; }
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
  return record ? digest([record.id, record.goal, record.stage, record.files ?? [], record.risks ?? [], record.checkout ?? null, ...(record.repositories ? [record.repositories] : []), ...(record.engineeringContract ? [engineeringContractDigest(record.engineeringContract)] : [])]) : null;
}
export function taskSource(repository) {
  return repository?.gitAvailable && repository.fingerprints?.complete
    ? digest([repository.head, repository.fingerprints.index, repository.fingerprints.workingTree]) : null;
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
  const acceptance = taskAcceptance(state, repository);
  if (stale || (record.engineeringContract && !acceptance.complete)) Object.assign(task, { status: 'working', goalComplete: false, verification: 'unknown', verificationRevision: undefined, safeBoundary: false });
  return { task, stale, acceptance, evidenceDigest: digest(record.acceptanceEvidence ?? null), taskContract: taskContract(record), binding: { kind: 'repository', identity: source }, summary: record.summary ?? null, nextStep: record.nextStep ?? null };
}
export function taskAcceptance(state, repository) {
  const record = state.taskRecord;
  return acceptanceCoverage(record?.engineeringContract, record?.acceptanceEvidence,
    { kind: 'repository', identity: taskSource(repository) }, unresolvedAttempts(state.attempts).length > 0);
}
export function recordTaskStep(state, input, repository, id, at) {
  keys(input, ['stage', 'target', 'approach', 'evidence', 'outcome', 'result', 'nextStep', 'complete', 'profile', 'maxAttempts', 'acceptanceEvidence', 'expectedTaskContract', 'expectedEvidenceDigest'], 'task step');
  const record = state.taskRecord;
  if (!record) throw new Error('task-step requires task-start for this session and task');
  stage(input.stage ?? record.stage);
  for (const field of ['result', 'nextStep']) if (!text(input[field])) throw new Error(`task step ${field} must be 1..512 characters without control characters`);
  if (input.complete !== undefined && typeof input.complete !== 'boolean') throw new Error('task completion must be boolean');
  if (input.complete && input.outcome !== 'passed') throw new Error('task completion requires a passed acceptance receipt');
  const selected = input.stage ?? record.stage;
  const receipt = attemptReceipt({ operation: selected, target: input.target, approach: input.approach, evidence: input.evidence, outcome: input.outcome,
    ...(input.profile !== undefined ? { profile: input.profile } : {}), ...(input.maxAttempts !== undefined ? { maxAttempts: input.maxAttempts } : {}) }, repository, id, at);
  if (input.acceptanceEvidence !== undefined) {
    if (!record.engineeringContract) throw new Error('acceptance evidence requires an engineering contract');
    if (input.expectedTaskContract !== taskContract(record)) throw new Error('acceptance evidence task contract changed; reobserve the task');
    if (input.expectedEvidenceDigest !== digest(record.acceptanceEvidence)) throw new Error('acceptance evidence ledger changed; reload before replacing evidence');
    validateAcceptanceEvidence(input.acceptanceEvidence);
    // Explicit whole-ledger replacement permits a documented correction without
    // choosing a passing receipt over a contradictory failure automatically.
    record.acceptanceEvidence = input.acceptanceEvidence;
  }
  appendAttempt(state, receipt);
  const acceptance = taskAcceptance(state, repository);
  if (input.complete && record.engineeringContract && !acceptance.complete) throw new Error('task completion requires current verified evidence for every required criterion and no open conflicts or decisions');
  if (input.complete && unresolvedAttempts(state.attempts).length) throw new Error('task completion has unresolved acceptance failures or blockers');
  Object.assign(record, { stage: selected, status: input.outcome === 'passed' ? 'completed' : input.outcome === 'quality-failure' ? 'failed' : input.outcome === 'prerequisite-resolved' ? 'working' : 'blocked',
    goalComplete: input.complete === true, revision: repository.head, source: receipt.source, safeBoundary: true,
    summary: input.result, nextStep: input.nextStep });
  if (selected === 'verify' && ['passed', 'quality-failure'].includes(input.outcome)) Object.assign(record, { verification: input.outcome === 'passed' ? 'passed' : 'failed', verificationRevision: repository.head });
  else { delete record.verification; delete record.verificationRevision; } // blockers and resolutions did not run the check
  record.acceptedContract = taskContract(record);
  if (input.outcome === 'passed' && !record.completed?.endsWith(input.result)) record.completed = [record.completed, input.result].filter(Boolean).join('\n').slice(-1024);
  const continuation = { goal: record.goal, currentState: input.result,
    completed: record.completed ?? '', approach: input.approach, evidence: input.evidence,
    filesChanged: record.files.join(', ').slice(0, 1024), validation: `${input.outcome}: ${input.evidence}`,
    remainingWork: input.complete ? '' : input.nextStep, blockers: record.status === 'blocked' ? input.result : '', nextStep: input.nextStep };
  validateMetadata({ taskId: record.id, continuation });
  return { state, receipt, continuation, acceptance };
}
