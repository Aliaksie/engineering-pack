import { sessionAdvice } from './advice.mjs';
import { attemptStatus } from './activity.mjs';
import { capabilityAdvice, modelAdvice } from '../policy/models.mjs';
import { taskContract } from './task.mjs';
import { digest } from '../cache/identity.mjs';
import { route } from '../routing/route.mjs';

const roleFor = { locate: 'locator', discover: 'locator', implement: 'implementer', verify: 'verifier', explore: 'verifier', review: 'reviewer', plan: 'planner', architecture: 'planner' };

// Read-only aggregation. Explicit session identity opts into existing pack state; no transcript.
export function sessionStatus(input = {}, project = { formatVersion: 1 }, { session, task, now, observations = {}, catalog } = {}) {
  if (session && input.sessionId && session !== input.sessionId) throw new Error('session input identity mismatch');
  if (task && input.task?.id && task !== input.task.id) throw new Error('task input identity mismatch');
  const sessionId = session ?? input.sessionId;
  const taskId = task ?? input.task?.id;
  const normalized = { ...input, ...(sessionId ? { sessionId } : {}), task: { ...input.task, ...(taskId ? { id: taskId } : {}) } };
  const diagnostics = [];
  let state, attempts, repositoryEvidence, checkpoint = { status: 'unknown', reason: 'No session checkpoint selected.' }, pack = null;
  if (sessionId && taskId) {
    try {
      if (observations.stateError) throw new Error(observations.stateError);
      state = observations.state;
      if (!state) throw new Error('No activity observation.');
      const operation = normalized.task.operation ?? normalized.task.stage ?? state.attempts.at(-1)?.operation;
      if (operation) normalized.task.operation = operation;
      if (!normalized.task.stage && Object.hasOwn(roleFor, operation)) normalized.task.stage = operation;
      if (state.attempts.length && operation) attempts = attemptStatus(state.attempts, operation);
      normalized.activity = { ...input.activity, ...state.activity };
      if (['locate', 'discover'].includes(operation) && state.lastDiscovery?.repetitions >= 3) normalized.activity.noProgressAttempts = state.lastDiscovery.repetitions;
      if (attempts) {
        Object.assign(normalized.activity, { attempts: attempts.attempts, maxAttempts: attempts.maxAttempts,
          noProgressAttempts: Math.max(attempts.noProgressAttempts, normalized.activity.noProgressAttempts ?? 0), retries: attempts.retries });
        if (attempts.blocked) normalized.task.status = 'blocked';
      }
    } catch (error) { diagnostics.push(`Activity unavailable: ${error.message}`); }
  }
  if (sessionId) {
    try {
      if (observations.checkpointError) throw new Error(observations.checkpointError);
      const saved = observations.saved;
      const continuation = saved?.continuation;
      const binding = saved?.continuationSource;
      const sameTask = Boolean(taskId && saved?.taskId === taskId && binding?.taskId === taskId);
      const recovered = continuation && sameTask ? observations.recovered
        : { saved, changed: null, changeDetection: { complete: false } };
      repositoryEvidence = recovered.current;
      const continuationUnchanged = binding?.complete === true
        && (!state?.taskRecord || (binding.taskContract === taskContract(state.taskRecord) && state.taskRecord.checkout === recovered.current?.checkout))
        && (binding.checkout ?? null) === (recovered.current?.checkout ?? null) && recovered.current?.head === binding.head
        && recovered.current?.fingerprints.index === binding.index && recovered.current?.fingerprints.workingTree === binding.workingTree;
      const fresh = Boolean(continuation && sameTask && continuationUnchanged && recovered.changed === false && recovered.changeDetection.complete);
      normalized.task.handoffSaved = Boolean(continuation && sameTask);
      normalized.task.checkpointFresh = fresh;
      checkpoint = { status: fresh ? 'fresh' : recovered.saved ? 'stale-or-incomplete' : 'missing',
        fileRevision: recovered.saved?.head ?? null, taskMatches: sameTask,
        changed: recovered.changed, complete: recovered.changeDetection.complete,
        nextStep: sameTask ? continuation?.nextStep ?? null : null,
        reason: fresh ? 'Task identity and complete repository fingerprints match the saved continuation.'
          : 'A matching bounded continuation and unchanged complete repository evidence are required.' };
    } catch (error) {
      normalized.task.handoffSaved = false; normalized.task.checkpointFresh = false;
      diagnostics.push(`Checkpoint unavailable: ${error.message}`);
    }
  } else {
    // A bare caller declaration cannot establish that a safe continuation exists on disk.
    normalized.task.handoffSaved = false; normalized.task.checkpointFresh = false;
  }
  if (normalized.task.stage) {
    try {
      if (observations.packError) throw new Error(observations.packError);
      pack = observations.pack;
      normalized.packContext = { estimatedTokens: pack.totals.estimatedTokens, skills: new Set(observations.plan.skills).size, references: new Set(observations.plan.references).size };
    } catch (error) { diagnostics.push(`Pack selection unavailable: ${error.message}`); }
  }
  const status = sessionAdvice(normalized, project, { now, catalog });
  if (pack) status.packContext = { ...status.packContext, scope: 'routed selected files; not observed host loads', source: 'context-report', complete: pack.complete };
  if (state) status.activity.scope = 'Engineering Pack command receipts plus declarations for unobserved fields; not provider usage';
  const capability = capabilityAdvice(normalized.task.operation ?? normalized.task.stage ?? 'unknown', normalized.task.risks ?? []);
  const mappings = project.execution?.hosts?.[normalized.host?.name]?.models ?? {};
  const matchingProfiles = normalized.host?.model ? Object.entries(mappings).filter(([, name]) => name === normalized.host.model).map(([profile]) => profile) : [];
  const currentProfile = matchingProfiles.length === 1 ? matchingProfiles[0] : undefined;
  capability.currentProfile = currentProfile ?? null;
  capability.currentClass = project.execution?.profiles?.[currentProfile]?.class ?? 'unknown';
  capability.recommendation = capability.class === 'economical' && ['standard', 'strong'].includes(capability.currentClass) ? 'use-deterministic-tool-or-economical-profile' : null;
  let model = null;
  const role = roleFor[normalized.task.operation] ?? (capability.class === 'economical' ? 'locator' : roleFor[normalized.task.stage]);
  if (role && taskId && normalized.task.revision && normalized.host?.name) {
    const records = attempts?.records ?? [];
    let currentSource = null;
    if (records.length && project.execution?.roles?.[role]?.profile) {
      repositoryEvidence ??= observations.repository;
      if (repositoryEvidence?.gitAvailable && repositoryEvidence.fingerprints.complete) currentSource = digest([repositoryEvidence.head, repositoryEvidence.fingerprints.index, repositoryEvidence.fingerprints.workingTree]);
    }
    const recordedAttempts = records.filter(record => record.profile && currentSource && record.source === currentSource && record.revision === normalized.task.revision
      && !['blocked', 'prerequisite-resolved'].includes(record.outcome)).map(record => ({ id: record.id, profile: record.profile, task: taskId,
      revision: record.revision, outcome: record.outcome, evidence: record.evidence }));
    model = modelAdvice({ host: normalized.host.name, role, task: taskId, revision: normalized.task.revision,
      capabilityClass: capability.class, ...(currentProfile ? { currentProfile } : {}), attempts: recordedAttempts }, project);
  }
  if (attempts?.blocked) {
    status.recommendation = { action: 'stop-blocked', reason: 'A recorded permission, tool or prerequisite blocker requires resolution.', effect: 'read', prerequisites: [] };
  } else if (model?.decision === 'stop' && (attempts?.attempts ?? 0) > 0) {
    status.recommendation = { action: 'return-control', reason: model.reasons.join(' '), effect: 'read', prerequisites: ['Configured quality/escalation limits are exhausted; preserve evidence and return control.'] };
  } else if (['return-control', 'retry-differently'].includes(status.recommendation?.action) && model?.decision === 'recommend'
    && attempts?.records.at(-1)?.profile && model.profile !== attempts.records.at(-1).profile
    && (model.mainModel?.switchSupported === true || model.workerMappingAvailable === true)) {
    status.recommendation = { action: 'escalate-model', reason: model.reasons.join(' '), effect: 'recommendation-only', prerequisites: ['Use only the configured host mapping and existing authorization; no switch was performed.'] };
  }
  if (['stop-blocked', 'escalate-model'].includes(status.recommendation?.action) || (model?.decision === 'stop' && (attempts?.attempts ?? 0) > 0)) {
    const at = now;
    const key = digest({ session: sessionId, task: normalized.task, suggestion: status.recommendation });
    const previousAt = Date.parse(input.previous?.at);
    status.suppressed = input.previous?.key === key && at >= previousAt && at - previousAt < (input.policy?.cooldownMs ?? 300000);
    status.notice = status.suppressed ? input.previous : { key, at: new Date(at).toISOString() };
  }
  if (['review', 'compact'].includes(status.recommendation?.action) && state?.recommendations) {
    repositoryEvidence ??= observations.repository;
    const source = repositoryEvidence?.gitAvailable && repositoryEvidence.fingerprints?.complete
      ? digest([repositoryEvidence.head, repositoryEvidence.fingerprints.index, repositoryEvidence.fingerprints.workingTree]) : null;
    const offer = Object.values(state.recommendations).findLast(value => value.action === status.recommendation.action && (value.action !== 'review' || value.scope === 'review:current-source') && value.source === source && value.policy === digest(project) && (value.action !== 'review' || JSON.stringify(value.risks ?? []) === JSON.stringify([...new Set(normalized.task.risks ?? [])].sort())));
    if (offer) {
      status.actionOffer = offer;
      status.suppressed = true;
      status.recommendation = { ...status.recommendation, approvalRequired: !['approved', 'already-authorized'].includes(offer.decision), decision: offer.decision, execution: offer.execution ?? 'not-observed' };
    }
  }
  status.suggestion = status.suppressed ? null : status.recommendation;
  return { ...status, checkpoint, capability, modelAdvice: model, discovery: state ? {
    requested: state.activity.discoveryRequests ?? null, reused: state.activity.reuseHits ?? null,
    fresh: state.activity.freshLookups ?? null, batches: state.activity.batchedOperations ?? null
  } : null, packReport: pack, attemptEvidence: attempts?.evidence ?? [], diagnostics,
  activitySource: state ? 'Engineering Pack command receipts; explicit declarations fill unobserved fields' : 'caller declarations; host-wide activity is unknown' };
}

// Describe only the observations this request needs. The shell fulfills them.
export function sessionObservationPlan(input, project, state, saved, catalog) {
  const operation = input.task?.operation ?? input.task?.stage ?? state?.attempts.at(-1)?.operation;
  const stage = input.task?.stage ?? (Object.hasOwn(roleFor, operation) ? operation : undefined);
  const role = roleFor[operation] ?? roleFor[stage];
  const sameTask = Boolean(input.task?.id && saved?.taskId === input.task.id && saved?.continuationSource?.taskId === input.task.id);
  let plan, packError;
  if (stage) {
    try { plan = route({ intent: { architecture:'investigate', story:'story', discover:'locate' }[stage] ?? stage,
      files: input.task?.files ?? [], risks: input.task?.risks ?? [], project, host: input.host?.name,
      verificationMode: input.task?.verificationMode, catalog }); }
    catch (error) { packError = error.message; }
  }
  return { plan, packError, recover: Boolean(saved?.continuation && sameTask),
    repository: Boolean(state?.attempts.length && operation && input.task?.id && input.task?.revision && input.host?.name && project.execution?.roles?.[role]?.profile) };
}
