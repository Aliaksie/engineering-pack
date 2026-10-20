import { decide } from '../engine-client.mjs';
import { readProject } from '../project/configuration.mjs';
import { route } from '../routing/select.mjs';
import { readSessionState, updateSessionState, addActivity, bindingFile, activeTask } from './activity.mjs';
import { sessionStatus } from './status.mjs';
import { saveCheckpoint } from '../checkpoint.mjs';
import { writeState, lockState, releaseState, repositoryState, clock, uniqueId } from '../../platform/index.mjs';

function checkpoint(root, session, task, project, continuation, expectedSource) {
  if (!project.checkpoint.enabled) return { status: 'disabled' };
  try { return saveCheckpoint(root, session, { taskId: task, ...(continuation ? { continuation } : {}) }, { expectedSource }); }
  catch (error) { return { status: 'unavailable', reason: error.message }; }
}
export function startTask(root, session, task, input, { host, preferencesFile } = {}) {
  decide('activity-identity', { session, task });
  const project = readProject(root, { preferencesFile });
  const file = bindingFile(root, session, true);
  const ticket = lockState(file);
  if (!ticket) throw new Error('active task writer is busy');
  try {
    const repository = repositoryState(root);
    const previous = readSessionState(root, session, task);
    const started = decide('task-start', { state: previous, input: { ...input, id: task }, repository });
    const routing = route({ intent: started.intent,
      files: started.state.taskRecord.files, project, host });
    const model = repository.head && ['locator', 'researcher', 'implementer', 'reviewer', 'planner', 'verifier'].includes(routing.execution.role)
      ? decide('model-advice', { input: { host: host ?? 'unknown', role: routing.execution.role, task, revision: repository.head, capabilityClass: routing.capability.class }, project })
      : { decision: 'unavailable', reasons: ['A supported role and source revision are required; retain current execution.'] };
    updateSessionState(root, session, task, state => {
      // Revalidate under the task-state lock; a concurrent step must not be overwritten.
      const current = decide('task-start', { state, input: { ...input, id: task }, repository });
      Object.assign(state, current.state); addActivity(state, { routingDecisions: 1 });
    });
    writeState(file, { formatVersion: 1, task }, 2048);
    return { session, task, routing, modelAdvice: model, checkpoint: checkpoint(root, session, task, project),
      authorization: 'Local task binding and snapshot only; no acceptance, worker, model switch or remote action was inferred.' };
  } finally { releaseState(ticket); }
}
export function stepTask(root, session, task, input, { host, preferencesFile, bundle } = {}) {
  decide('activity-identity', { session, task });
  if (activeTask(root, session) !== task) throw new Error('task-step requires the active task binding; use task-start to switch tasks');
  const ticket = lockState(bindingFile(root, session));
  if (!ticket) throw new Error('active task writer is busy');
  try {
    if (activeTask(root, session) !== task) throw new Error('task-step requires the active task binding; use task-start to switch tasks');
    const project = readProject(root, { preferencesFile });
    const repository = repositoryState(root);
    let result;
    updateSessionState(root, session, task, state => {
      const profileLimit = project.execution?.profiles?.[input.profile]?.maxAttempts;
      result = decide('task-step', { state, input: { maxAttempts: profileLimit ?? project.efficiency.attempts.maxAttempts, ...input },
        repository, id: uniqueId(), at: clock().iso });
      Object.assign(state, result.state);
    });
    const saved = checkpoint(root, session, task, project, result.continuation, result.receipt.source);
    let status;
    try { status = sessionStatus(host ? { host: { name: host } } : {}, project, { root, session, task, bundle }); }
    catch (error) { status = { diagnostic: error.message }; }
    let actionOffer;
    if (status.recommendation?.action === 'review' && project.efficiency.review.mode !== 'manual') {
      try {
        updateSessionState(root, session, task, state => {
          const priorAuthorization = state.taskRecord?.reviewAuthorization;
          const proposed = decide('action-propose', { state, project, repository, status, session, now: clock().ms,
            input: { action: 'review', scope: 'review:current-source', benefit: 'Review the verified changes for declared risks.', evidence: result.receipt.evidence, ...(priorAuthorization ? { priorAuthorization } : {}) },
            facts: { observations: { task: status.task } } });
          Object.assign(state, proposed.state); actionOffer = proposed;
        });
      } catch (error) { status.actionDiagnostic = error.message; }
    }
    if (status.notice) {
      try { updateSessionState(root, session, task, state => { state.notice = status.notice; }); }
      catch (error) { status.noticeDiagnostic = error.message; }
    }
    return { session, task, recorded: result.receipt, result: input.result, complete: result.state.taskRecord.goalComplete,
      checkpoint: saved, ...(actionOffer ? { actionOffer: { offer: actionOffer.offer, suppressed: actionOffer.suppressed, applicability: actionOffer.applicability, executed: false } } : {}),
      nextStep: actionOffer?.suppressed ? null : status.suggestion ?? null, status,
      authorization: 'Agent-declared acceptance recorded locally; no recommended action was executed.' };
  } finally { releaseState(ticket); }
}

export function taskAction(root, session, task, operation, input, facts = {}, { preferencesFile, bundle } = {}) {
  decide('activity-identity', { session, task });
  const ticket = lockState(bindingFile(root, session));
  if (!ticket) throw new Error('active task writer is busy');
  try {
    if (activeTask(root, session) !== task) throw new Error('action decisions require the active task binding');
    decide('action-facts', { facts });
    const project = readProject(root, { preferencesFile });
    const repository = repositoryState(root);
    const status = operation === 'action-decide' ? {} : sessionStatus(facts.observations ?? {}, project, { root, session, task, bundle });
    let result;
    const evaluate = state => {
      if (!state.taskRecord) throw new Error('action recommendations require task-start');
      return decide(operation, { state, input, facts, project, repository, status, session, now: clock().ms });
    };
    if (operation === 'action-check') result = evaluate(readSessionState(root, session, task));
    else updateSessionState(root, session, task, state => { result = evaluate(state); Object.assign(state, result.state); });
    const { state, ...response } = result;
    return response;
  } finally { releaseState(ticket); }
}
