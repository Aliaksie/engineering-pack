import { decide } from '../engine-client.mjs';
import { taskRepository, activeTask, bindingFile } from './activity.mjs';
import { lockState, releaseState } from '../../platform/index.mjs';

// Serialize native admission with task-start. Authorization happens before this lock;
// the binding and source are checked again after every asynchronous prerequisite.
export async function withTaskAdmission(input, effect) {
  const { root, session, task, source } = input;
  const ticket = lockState(bindingFile(root, session));
  if (!ticket) throw new Error('active task writer is busy');
  const assertCurrent = () => {
    if (activeTask(root, session) !== task) throw new Error('native effect requires the active task binding');
    if (source !== undefined && decide('task-source', { repository: taskRepository(root, session, task) }) !== source) throw new Error('native source changed after admission');
  };
  try { assertCurrent(); return await effect(assertCurrent); }
  finally { releaseState(ticket); }
}
