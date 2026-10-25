import { engineeringContractDigest, acceptanceCoverage } from './acceptance.mjs';
import { activityIdentity, unresolvedAttempts } from './activity.mjs';
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
