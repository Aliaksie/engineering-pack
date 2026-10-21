import { decide } from '../engine-client.mjs';
import { taskRepository, activeTask, bindingFile, readSessionState, updateSessionState } from './activity.mjs';
import { lockState, releaseState, clock } from '../../platform/index.mjs';

export function taskFeatures(root, session, task, input) {
  decide('activity-identity', { session, task });
  if (!input) return decide('feature-report', { state: readSessionState(root, session, task), repository: taskRepository(root, session, task) });
  const ticket = lockState(bindingFile(root, session));
  if (!ticket) throw new Error('active task writer is busy');
  try {
    if (activeTask(root, session) !== task) throw new Error('feature outcome requires the active task binding');
    const repository = taskRepository(root, session, task);
    let recorded;
    const state = updateSessionState(root, session, task, current => {
      const result = decide('feature-record', { state: current, input, repository, at: clock().iso, provenance: 'agent-declared' });
      Object.assign(current, result.state); recorded = result.recorded;
    });
    return { recorded, ...decide('feature-report', { state, repository }) };
  } finally { releaseState(ticket); }
}
