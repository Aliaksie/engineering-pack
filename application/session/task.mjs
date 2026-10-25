import { decide } from '../engine-client.mjs';
import { readProject } from '../project/configuration.mjs';
import { readSessionState, updateSessionState, bindingFile, activeTask, taskRepository } from './activity.mjs';
import { workspaceBoundary } from '../workspace.mjs';
import { sessionStatus } from './status.mjs';
import { saveCheckpoint } from '../checkpoint.mjs';
import { lockState, releaseState, clock, uniqueId } from '../../platform/index.mjs';

function checkpoint(root, session, task, project, continuation, expectedSource) {
  if (!project.checkpoint.enabled) return { status: 'disabled' };
  try { return saveCheckpoint(root, session, { taskId: task, taskContract: decide('task-contract', { record: readSessionState(root, session, task).taskRecord }), ...(continuation ? { continuation } : {}) }, { expectedSource }); }
  catch (error) { return { status: 'unavailable', reason: error.message }; }
}
export function stepTask(root, session, task, input, { host, preferencesFile, bundle } = {}) {
  decide('activity-identity', { session, task });
  if (activeTask(root, session) !== task) throw new Error('task-step requires the active task binding; use task-start to switch tasks');
  const ticket = lockState(bindingFile(root, session));
  if (!ticket) throw new Error('active task writer is busy');
  try {
    if (activeTask(root, session) !== task) throw new Error('task-step requires the active task binding; use task-start to switch tasks');
    const project = readProject(root, { preferencesFile });
    const repository = taskRepository(root, session, task);
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
    const complete = result.state.taskRecord.goalComplete && (!result.state.taskRecord.engineeringContract || status.task?.goalComplete === true);
    const workspace = workspaceBoundary(root, session, task, complete ? 'complete' : 'touch', project);
    status.workspace = workspace;
    return { session, task, recorded: result.receipt, result: input.result, complete, acceptance: status.acceptance ?? result.acceptance, workspace,
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
    const repository = taskRepository(root, session, task);
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
