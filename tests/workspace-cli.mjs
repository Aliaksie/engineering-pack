import { run } from './craft.mjs';

export function workspaceBoundary(root, session, task, action) {
  try {
    if (action === 'complete') return run(['task-step', '--root', root, '--session', session, '--task', task,
      '--stage', 'verify', '--target', 'workspace:fixture', '--approach', 'fixture', '--evidence', 'checks:fixture',
      '--outcome', 'passed', '--result', 'Fixture complete', '--next', 'Return', '--complete']).workspace;
    return run(['session', '--root', root, '--session', session, '--task', task]).workspace;
  } catch (error) { return { status: 'retained', reason: error.message }; }
}
export function workspaceOperation(root, action) {
  if (action === 'maintain') return run(['workspace-clean', '--root', root]);
  return workspaceBoundary(root, 'session', 'task', action);
}
