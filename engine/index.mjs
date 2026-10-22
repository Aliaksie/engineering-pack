import { groundingRequest, composeTaskContext } from './session/grounding.mjs';
import { actionFacts, proposeAction, decideAction, checkAction, claimAction, finishAction } from './session/recommendations.mjs';
import { validateTaskContext, validateEngineeringContract, engineeringContractDigest, validateAcceptanceEvidence, acceptanceCoverage } from './session/acceptance.mjs';
import { workerBudget } from './routing/worker-budget.mjs';
import { workerInput, workerState, workerSnapshot, workerAdmission, workerTransition, reconcileWorkers } from './session/workers.mjs';
import { recordFeature, featureReport } from './session/features.mjs';
import { taskBinding, taskContract, startTask, taskContext, taskSource, recordTaskStep } from './session/task.mjs';
import { effectiveSettings } from './policy/effective-settings.mjs';
import { inboxStatus, inboxPolicy } from './policy/inbox.mjs';
import { validateSnapshot, validateReview, publicationPlan, reviewPublicationPolicy } from './policy/review.mjs';
import { discoveryRequest, discoveryResult, discoveryState, deferredDiscovery } from './discovery/plan.mjs';
import { locateRequest, matchLines } from './discovery/match.mjs';
import { route, lookup } from './routing/route.mjs';
import { validateProject } from './contracts/project.mjs';
import { validatePlaybookCatalog } from './contracts/catalog.mjs';
import { planOperation } from './routing/operations.mjs';
import { prepareServices } from './routing/services.mjs';
import { modelAdvice, capabilityAdvice } from './policy/models.mjs';
import { sessionAdvice } from './session/advice.mjs';
import { sessionStatus, sessionObservationPlan } from './session/status.mjs';
import { validateMetadata, validateCheckpoint, checkpointState, checkpointUnchanged, compareCheckpoint } from './checkpoint.mjs';
import { activityUpdate, appendAttempt, attemptReceipt, attemptStatus, validateActivityState, activityIdentity } from './session/activity.mjs';
import { contextRequests, summarizeContext } from './session/context.mjs';
import { contextDeliveryPlan, contextDelivery, deliveryFailure } from './policy/context-delivery.mjs';
import { hintValid, validateHint, hintEligible, lookupReuse } from './cache/reuse.mjs';
import { digest, hash } from './cache/identity.mjs';
import { normalizeScope } from './contracts/paths.mjs';
import { repositoryScopes, repositorySnapshot, repositoryCheckout } from './repositories.mjs';

const operations = {
  'grounding-request': a => groundingRequest(a),
  'task-context-compose': a => composeTaskContext(a),
  'task-context-validate': a => validateTaskContext(a.context),
  'engineering-contract-validate': a => validateEngineeringContract(a.contract),
  'engineering-contract-digest': a => engineeringContractDigest(a.contract),
  'acceptance-evidence-validate': a => validateAcceptanceEvidence(a.evidence),
  'acceptance-coverage': a => acceptanceCoverage(a.contract, a.evidence, a.current, a.blockers),
  'repository-scopes': a => repositoryScopes(a.repositories),
  'repository-snapshot': a => repositorySnapshot(a.repositories),
  'repository-checkout': a => repositoryCheckout(a.repositories),
  'checkpoint-validate': a => validateCheckpoint(a.state),
  'hint-validate': a => validateHint(a.state),
  'delivery-default': () => ({ decision: 'allow' }),
  'inbox-policy': a => inboxPolicy(a),
  'inbox-status': a => inboxStatus(a),
  'review-snapshot': a => validateSnapshot(a.snapshot),
  'review-validate': a => validateReview(a.snapshot, a.review),
  'review-publication': a => publicationPlan(a.snapshot, a.review, a.target),
  'worker-budget': a => workerBudget(a.project, a.facts, a.options),
  'feature-record': a => recordFeature(a.state, a.input, Object.hasOwn(a, 'source') ? a.source : taskSource(a.repository), a.at, a.provenance),
  'feature-report': a => featureReport(a.state, taskSource(a.repository)),
  'worker-input': a => workerInput(a.input),
  'worker-state': a => workerState(a.state, a.tree),
  'worker-snapshot': a => workerSnapshot(a.snapshot, a.session, a.now),
  'worker-admit': a => workerAdmission(a),
  'worker-transition': a => workerTransition(a.state, a.id, a.result),
  'worker-reconcile': a => reconcileWorkers(a.state, a.snapshot, a.targets),
  'action-facts': a => actionFacts(a.facts),
  'action-propose': a => proposeAction(a),
  'action-decide': a => decideAction(a),
  'action-check': a => checkAction(a),
  'action-claim': a => claimAction(a),
  'action-finish': a => finishAction(a),
  'task-binding': a => taskBinding(a.state, a.repository),
  'task-contract': a => taskContract(a.record),
  'task-start': a => startTask(a.state, a.input, a.repository),
  'task-source': a => taskSource(a.repository),
  'task-context': a => taskContext(a.state, a.repository),
  'task-step': a => recordTaskStep(a.state, a.input, a.repository, a.id, a.at),
  'activity-validate': a => validateActivityState(a.state),
  'activity-identity': a => activityIdentity(a.session, a.task),
  'discovery-request': a => discoveryRequest(a.input),
  'discovery-result': a => discoveryResult(a),
  'discovery-state': a => discoveryState(a.current, a.input),
  'discovery-deferred': a => deferredDiscovery(a.request),
  'locate-request': a => locateRequest(a.input),
  'locate-match': a => matchLines(a.content, a.file, a.terms, a.maxMatches),
  route: a => route({ ...a, catalog: a.catalog }),
  lookup: a => lookup(a.project, a.options),
  'effective-settings': a => effectiveSettings({ ...a, project: validateProject(a.project) }),
  'validate-project': a => validateProject(a.project),
  'validate-catalog': a => validatePlaybookCatalog(a.catalog),
  operation: a => planOperation(a.project, a.operation, a.params, a.bindings),
  services: a => prepareServices(a.project, { ...a.options, bindings: a.bindings }),
  'model-advice': a => modelAdvice(a.input, a.project),
  capability: a => capabilityAdvice(a.operation, a.risks),
  'session-advice': a => sessionAdvice(a.input, a.project, { ...a.options, catalog: a.catalog }),
  'session-status': a => sessionStatus(a.input, a.project, { ...a.options, catalog: a.catalog }),
  'session-observation-plan': a => sessionObservationPlan(a.input, a.project, a.state, a.saved, a.catalog),
  'checkpoint-metadata': a => validateMetadata(a.metadata),
  'checkpoint-unchanged': a => checkpointUnchanged(a.previous, a.state),
  'checkpoint-state': a => checkpointState(a.previous, a.metadata, a.repository, a.updated),
  'checkpoint-compare': a => compareCheckpoint(a.saved, a.current),
  'activity-update': a => activityUpdate(a.state, a.values),
  'attempt-receipt': a => attemptReceipt(a.attempt, a.repository, a.id, a.at),
  'attempt-append': a => appendAttempt(a.state, a.receipt),
  'attempt-status': a => attemptStatus(a.records, a.operation),
  'context-requests': a => contextRequests(a.plan),
  'context-summary': a => summarizeContext(a.entries, a.missing, a.budgetBytes),
  'context-delivery-plan': a => contextDeliveryPlan(a.facts, a.project),
  'review-publication-policy': a => reviewPublicationPolicy(a),
  'context-delivery': a => contextDelivery(a),
  'delivery-failure': a => deliveryFailure(a.enforcementRequired, a.message),
  'hint-eligible': a => hintEligible(a.event, a.result),
  'hint-valid': a => hintValid(a.previous, a.key, a.now),
  'lookup-reuse': a => lookupReuse(a.terms, a.paths, a.identity, a.state),
  digest: a => digest(a.value), hash: a => hash(a.text),
  'locate-scope': a => normalizeScope(a.value)
};

function json(value) {
  if (value === null || typeof value === 'string' || typeof value === 'boolean') return;
  if (typeof value === 'number' && Number.isFinite(value)) return;
  if (Array.isArray(value)) { for (const entry of value) json(entry); return; }
  if (value && Object.getPrototypeOf(value) === Object.prototype) { for (const entry of Object.values(value)) json(entry); return; }
  throw new Error('Engine requests must contain only JSON values');
}

// Public, synchronous JSON contract. No ports, callbacks or host objects cross this seam.
// A native implementation can replace this function without changing application I/O.
export function evaluate(request) {
  try {
    json(request);
    if (request.formatVersion !== 1) return { formatVersion: 1, ok: false, error: { code: 'UNSUPPORTED_VERSION', message: 'unsupported engine formatVersion' } };
    if (!Object.hasOwn(operations, request.operation)) return { formatVersion: 1, ok: false, error: { code: 'UNKNOWN_OPERATION', message: `unknown engine operation: ${request.operation}` } };
    const args = JSON.parse(JSON.stringify(request.input ?? {}));
    const result = operations[request.operation](args);
    return JSON.parse(JSON.stringify({ formatVersion: 1, ok: true, result }));
  } catch (error) {
    return { formatVersion: 1, ok: false, error: { code: 'INVALID_INPUT', message: error.message } };
  }
}
