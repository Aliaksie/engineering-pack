import { unresolvedAttempts } from './activity.mjs';
import { digest } from '../cache/identity.mjs';
import { taskSource, taskContract } from './task.mjs';
import { workerBudget } from '../routing/worker-budget.mjs';
import { delegation } from '../routing/execution.mjs';

const text = value => typeof value === 'string' && value.length > 0 && value.length <= 512 && !/[\x00-\x1f\x7f]/.test(value);
const actions = ['review', 'delegate', 'compact'];
function keys(value, allowed, label) {
  if (!value || typeof value !== 'object' || Array.isArray(value) || Object.keys(value).some(key => !allowed.includes(key))) throw new Error(`invalid ${label}`);
}
export function actionFacts(facts = {}) {
  keys(facts, ['observations', 'workers', 'independentTasks', 'allowed', 'supported', 'reuseAvailable'], 'action facts');
  if (facts.independentTasks !== undefined && (!Number.isSafeInteger(facts.independentTasks) || facts.independentTasks < 0 || facts.independentTasks > 64)) throw new Error('invalid independent task count');
  for (const field of ['allowed', 'supported', 'reuseAvailable']) if (facts[field] !== undefined && typeof facts[field] !== 'boolean') throw new Error(`invalid action ${field}`);
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
  const reusable = facts.reuseAvailable === true && budget.effectiveLimit > 0 && budget.occupiedTaskWorkers > 0 && budget.occupiedTaskWorkers <= budget.effectiveLimit;
  if (budget.available < 1 && !reusable) return { ...fail('No authorized, observed worker capacity is available.'), budget };
  if ((facts.independentTasks ?? 0) < 2) return { ...fail('Bounded independent work does not justify delegation.'), budget };
  const profile = input.profile ?? project.execution?.roles?.[input.role]?.profile;
  if (!profile) return { ...fail('A configured logical worker profile is required.'), budget };
  const execution = delegation(project, input.role, facts.observations?.host?.name, profile);
  if (execution.mode !== 'delegate') return { ...fail('An exact supported host/role/model mapping is unavailable.'), budget };
  return { eligible: true, reason: 'Independent assignments fit the shared task-wide worker ceiling.', budget, execution, workerCount: Math.min(facts.independentTasks, reusable ? Math.max(1, budget.available) : budget.available) };
}
export function proposeAction({ state, input, project, repository, status, facts = {}, session, now }) {
  keys(input, ['action', 'scope', 'benefit', 'evidence', 'role', 'profile', 'priorAuthorization'], 'action proposal');
  if (!actions.includes(input.action) || !['scope', 'benefit', 'evidence'].every(field => text(input[field]))) throw new Error('action, bounded scope, benefit and evidence are required');
  for (const field of ['role', 'profile', 'priorAuthorization']) if (input[field] !== undefined && !text(input[field])) throw new Error(`invalid action ${field}`);
  actionFacts(facts);
  const applicable = eligibility(input.action, input, state, project, repository, status, facts, session, now);
  if (!applicable.eligible) return { state, offer: null, suppressed: true, applicability: applicable, executed: false };
  const source = taskSource(repository), policy = digest(project), contract = taskContract(state.taskRecord);
  const risks = [...new Set(facts.observations?.task?.risks ?? [])].sort();
  const host = facts.observations?.host?.name ?? null;
  const model = applicable.execution?.model ?? null;
  const id = digest([state.taskRecord.id, input.action, input.scope, source, policy, input.role ?? null, applicable.execution?.profile ?? input.profile ?? null, risks, host, model, contract]);
  state.recommendations ??= {};
  const existing = state.recommendations[id];
  if (existing) return { state, offer: existing, suppressed: true, applicability: applicable, executed: false };
  if (Object.keys(state.recommendations).length >= 16) {
    const old = Object.entries(state.recommendations).find(([, value]) => !value.executionReceipt && (value.source !== source || value.taskContract !== contract || value.decision === 'declined'));
    if (!old) throw new Error('task recommendation capacity reached; resolve existing proposals before adding more');
    delete state.recommendations[old[0]];
  }
  const offer = { id, action: input.action, scope: input.scope, benefit: input.benefit, evidence: input.evidence,
    source, policy, taskContract: contract, risks, host, model, role: input.role ?? null, profile: applicable.execution?.profile ?? input.profile ?? null,
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
  if ((offer.taskContract !== undefined || state.taskRecord?.engineeringContract)
    && offer.taskContract !== taskContract(state.taskRecord)) throw new Error('action proposal is stale; task contract changed or its binding is unavailable');
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

export function claimAction(args) {
  const checked = checkAction(args), offer = checked.offer;
  if (!['review', 'compact'].includes(offer.action)) throw new Error('worker execution uses the shared worker controller');
  if (offer.executionReceipt) return { state: args.state, receipt: offer.executionReceipt, duplicate: true };
  if (!checked.ready) throw new Error(checked.reason);
  if (offer.action === 'compact' && !/^[a-f0-9]{64}$/.test(args.checkpoint ?? '')) throw new Error('compaction requires a bound checkpoint identity');
  offer.executionReceipt = { key: digest([offer.id, 'execute']), state: 'dispatching', source: offer.source,
    checkpoint: args.checkpoint ?? null, generation: 0, evidence: null, acceptanceEvidence: null };
  offer.execution = 'dispatching';
  return { state: args.state, receipt: offer.executionReceipt, duplicate: false };
}
export function finishAction({ state, id, key, generation, result }) {
  const offer = Object.hasOwn(state.recommendations ?? {}, id) ? state.recommendations[id] : null;
  const receipt = offer?.executionReceipt;
  if (!receipt || receipt.key !== key) throw new Error('unknown action execution');
  if (receipt.generation !== generation || ['completed', 'not-started'].includes(receipt.state)) return { state, receipt, ignored: true };
  keys(result, ['state', 'evidence', 'source', 'checkpoint', 'resumed', 'acceptanceEvidence', 'fenced'], 'native action result');
  if (!['started', 'completed', 'not-started', 'uncertain'].includes(result.state)) throw new Error('invalid native action state');
  for (const field of ['evidence', 'acceptanceEvidence']) if (result[field] !== undefined && result[field] !== null && !text(result[field])) throw new Error(`invalid native action ${field}`);
  if ((result.source !== undefined && result.source !== receipt.source)
    || (result.checkpoint !== undefined && result.checkpoint !== receipt.checkpoint)
    || (result.resumed !== undefined && typeof result.resumed !== 'boolean') || (result.fenced !== undefined && typeof result.fenced !== 'boolean')) throw new Error('invalid native action binding');
  if (result.state !== 'uncertain' && (!text(result.evidence) || result.source !== receipt.source)) throw new Error('native action result must identify its actual source/evidence');
  if (result.state === 'not-started' && result.fenced !== true) throw new Error('negative action lookup must fence late execution');
  if (result.state === 'completed' && offer.action === 'compact'
    && (result.checkpoint !== receipt.checkpoint || result.resumed !== true || !text(result.acceptanceEvidence))) throw new Error('compaction completion requires matching resumed checkpoint and acceptance evidence');
  Object.assign(receipt, { state: result.state, evidence: result.evidence ?? null, acceptanceEvidence: result.acceptanceEvidence ?? null, generation: receipt.generation + 1 });
  offer.execution = result.state === 'completed' ? 'host-observed' : result.state;
  return { state, receipt, ignored: false };
}
