import { unresolvedAttempts } from './activity.mjs';
import { digest } from '../cache/identity.mjs';
import { taskSource } from './task.mjs';
import { workerBudget } from '../routing/worker-budget.mjs';
import { delegation } from '../routing/execution.mjs';

const text = value => typeof value === 'string' && value.length > 0 && value.length <= 512 && !/[\x00-\x1f\x7f]/.test(value);
const actions = ['review', 'delegate', 'compact'];
function keys(value, allowed, label) {
  if (!value || typeof value !== 'object' || Array.isArray(value) || Object.keys(value).some(key => !allowed.includes(key))) throw new Error(`invalid ${label}`);
}
export function actionFacts(facts = {}) {
  keys(facts, ['observations', 'workers', 'independentTasks', 'allowed', 'supported'], 'action facts');
  if (facts.independentTasks !== undefined && (!Number.isSafeInteger(facts.independentTasks) || facts.independentTasks < 0 || facts.independentTasks > 64)) throw new Error('invalid independent task count');
  for (const field of ['allowed', 'supported']) if (facts[field] !== undefined && typeof facts[field] !== 'boolean') throw new Error(`invalid action ${field}`);
  return facts;
}
function eligibility(action, input, state, project, repository, status, facts, session, now) {
  const source = taskSource(repository);
  const fail = reason => ({ eligible: false, reason });
  if (!source) return fail('Complete repository evidence is unavailable.');
  if (facts.allowed === false) return fail('Current session or consumer policy prohibits this action.');
  if (facts.supported === false) return fail('The host does not support this action.');
  if (action === 'review') {
    if (project.efficiency?.review?.mode === 'manual') return fail('Automatic review recommendations are disabled.');
    const record = state.taskRecord;
    const verified = record?.source === source && record.verification === 'passed' && !status.taskEvidenceStale;
    if (!input.priorAuthorization && !facts.observations?.task?.reviewRequested && !(verified && facts.observations?.task?.risks?.length)) return fail('Review needs a requested scope or current verified acceptance and a declared risk.');
    return { eligible: true, reason: 'A scoped review is requested or risk justifies an additional review of verified source.' };
  }
  if (action === 'compact') {
    if (project.efficiency?.context?.action === 'manual') return fail('Automatic context recommendations are disabled.');
    if (facts.observations?.host?.reliableCompaction !== true) return fail('Reliable host compaction is unsupported or unverified.');
    if (status.task?.goalComplete) return fail('The task is complete; return its outcome rather than changing context.');
    if (!(status.task?.safeBoundary === true || (status.task?.safeBoundary === undefined && status.task?.status === 'completed'))) return fail('Compaction requires a safe task boundary.');
    if (status.task?.almostComplete) return fail('Finish the almost-complete task before changing context.');
    if (!['high', 'saturated'].includes(status.pressure) || status.checkpoint?.status !== 'fresh') return fail('Fresh observed pressure and a matching fresh continuation checkpoint are required.');
    return { eligible: true, reason: 'Fresh host occupancy and a fresh continuation support compaction at this boundary.' };
  }
  if (project.efficiency?.delegation?.mode === 'manual') return fail('Automatic worker recommendations are disabled.');
  if (unresolvedAttempts(state.attempts).some(value => ['permission-denied', 'tool-unavailable', 'blocked'].includes(value.outcome))) return fail('Resolve the recorded prerequisite; workers cannot bypass a permission or tool blocker.');
  const budget = workerBudget(project, facts.workers, { session, now });
  if (budget.available < 1) return { ...fail('No authorized, observed worker capacity is available.'), budget };
  if ((facts.independentTasks ?? 0) < 2) return { ...fail('Bounded independent work does not justify delegation.'), budget };
  const profile = input.profile ?? project.execution?.roles?.[input.role]?.profile;
  if (!profile) return { ...fail('A configured logical worker profile is required.'), budget };
  const execution = delegation(project, input.role, facts.observations?.host?.name, profile);
  if (execution.mode !== 'delegate') return { ...fail('An exact supported host/role/model mapping is unavailable.'), budget };
  return { eligible: true, reason: 'Independent assignments fit the shared task-wide worker ceiling.', budget, execution, workerCount: Math.min(facts.independentTasks, budget.available) };
}
export function proposeAction({ state, input, project, repository, status, facts = {}, session, now }) {
  keys(input, ['action', 'scope', 'benefit', 'evidence', 'role', 'profile', 'priorAuthorization'], 'action proposal');
  if (!actions.includes(input.action) || !['scope', 'benefit', 'evidence'].every(field => text(input[field]))) throw new Error('action, bounded scope, benefit and evidence are required');
  for (const field of ['role', 'profile', 'priorAuthorization']) if (input[field] !== undefined && !text(input[field])) throw new Error(`invalid action ${field}`);
  actionFacts(facts);
  const applicable = eligibility(input.action, input, state, project, repository, status, facts, session, now);
  if (!applicable.eligible) return { state, offer: null, suppressed: true, applicability: applicable, executed: false };
  const source = taskSource(repository), policy = digest(project);
  const risks = [...new Set(facts.observations?.task?.risks ?? [])].sort();
  const host = facts.observations?.host?.name ?? null;
  const model = applicable.execution?.model ?? null;
  const id = digest([state.taskRecord.id, input.action, input.scope, source, policy, input.role ?? null, applicable.execution?.profile ?? input.profile ?? null, risks, host, model]);
  state.recommendations ??= {};
  const existing = state.recommendations[id];
  if (existing) return { state, offer: existing, suppressed: true, applicability: applicable, executed: false };
  if (Object.keys(state.recommendations).length >= 16) {
    const old = Object.entries(state.recommendations).find(([, value]) => value.source !== source || value.decision === 'declined');
    if (!old) throw new Error('task recommendation capacity reached; resolve existing proposals before adding more');
    delete state.recommendations[old[0]];
  }
  const offer = { id, action: input.action, scope: input.scope, benefit: input.benefit, evidence: input.evidence,
    source, policy, risks, host, model, role: input.role ?? null, profile: applicable.execution?.profile ?? input.profile ?? null,
    decision: input.priorAuthorization ? 'already-authorized' : 'pending',
    decisionEvidence: input.priorAuthorization ?? null, at: new Date(now).toISOString(),
    execution: 'not-observed', trigger: applicable.reason,
    ...(applicable.budget ? { workerBudget: applicable.budget, proposedWorkers: applicable.workerCount } : {}) };
  state.recommendations[id] = offer;
  return { state, offer, suppressed: false, applicability: applicable, executed: false };
}
function offerFor(state, id, project, repository) {
  if (!/^[a-f0-9]{64}$/.test(id ?? '') || !state.recommendations?.[id]) throw new Error('unknown action proposal');
  const offer = state.recommendations[id];
  if (offer.source !== taskSource(repository) || offer.policy !== digest(project)) throw new Error('action proposal is stale; source or effective policy changed');
  return offer;
}
export function decideAction({ state, input, project, repository, now }) {
  keys(input, ['id', 'decision', 'evidence'], 'action decision');
  if (!['approved', 'declined'].includes(input.decision) || !text(input.evidence)) throw new Error('explicit approval/decline and decision evidence are required');
  const offer = offerFor(state, input.id, project, repository);
  Object.assign(offer, { decision: input.decision, decisionEvidence: input.evidence, decidedAt: new Date(now).toISOString() });
  return { state, offer, executed: false, authorization: 'Caller-declared decision receipt; no host permission or completed execution is implied.' };
}
export function checkAction({ state, input, project, repository, status, facts = {}, session, now }) {
  keys(input, ['id'], 'action check'); actionFacts(facts);
  const offer = offerFor(state, input.id, project, repository);
  if (offer.host && facts.observations?.host?.name !== offer.host) return { offer, ready: false, executed: false, reason: 'Host identity changed or is unavailable; re-evaluate this action scope.' };
  const applicable = eligibility(offer.action, { ...offer, priorAuthorization: offer.decisionEvidence }, state, project, repository, status, facts, session, now);
  const approved = ['approved', 'already-authorized'].includes(offer.decision);
  return { offer, applicability: applicable, workerLimit: offer.action === 'delegate' ? Math.min(offer.proposedWorkers ?? 0, applicable.workerCount ?? 0) : null, ready: approved && applicable.eligible && facts.allowed === true && facts.supported === true,
    executed: false, authorization: 'Decision and host facts are caller declarations, not a permission grant; verify the real conversation and host policy.', reason: !approved ? `Action remains ${offer.decision}; silence is not approval.` : !applicable.eligible ? applicable.reason
      : facts.allowed !== true || facts.supported !== true ? 'Fresh session policy and host support must be explicitly verified.' : 'Scoped approval and current prerequisites are present; the host must perform and observe the action separately.' };
}
