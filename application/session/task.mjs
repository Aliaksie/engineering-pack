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
    updateSessionState(root, session, task, state => {
      // Revalidate under the task-state lock; a concurrent step must not be overwritten.
      const current = decide('task-start', { state, input: { ...input, id: task }, repository });
      Object.assign(state, current.state); addActivity(state, { routingDecisions: 1 });
    });
    writeState(file, { formatVersion: 1, task }, 2048);
    return { session, task, routing, checkpoint: checkpoint(root, session, task, project),
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
    if (status.notice) {
      try { updateSessionState(root, session, task, state => { state.notice = status.notice; }); }
      catch (error) { status.noticeDiagnostic = error.message; }
    }
    return { session, task, recorded: result.receipt, result: input.result, complete: result.state.taskRecord.goalComplete,
      checkpoint: saved, nextStep: status.suggestion ?? null, status,
      authorization: 'Agent-declared acceptance recorded locally; no recommended action was executed.' };
  } finally { releaseState(ticket); }
}
