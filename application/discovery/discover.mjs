import { diagnostic, withTelemetry, forwardTelemetry, commandTelemetry } from '../telemetry/observe.mjs';
import { decide } from '../engine-client.mjs';
import { nativeCommand, readState } from '../../platform/index.mjs';
import { readSessionState, activeTask, bindingFile } from '../session/activity.mjs';

// Session binding and validation remain with W2. Rust owns discovery and writes only
// when the locked state still equals this validated observation.
export function discover(root, input = {}, { session, task, preferencesFile } = {}) {
  return withTelemetry(root, { session }, () => locateCommand(root, { session, task, preferences: preferencesFile }, input));
}

function observation(root, session, task) {
  const bridge = {};
  if (session && !task) {
    try { task = activeTask(root, session); }
    catch { diagnostic('state', 'STATE_UNAVAILABLE'); }
  }
  if (session && task) {
    try {
      bridge.state = readSessionState(root, session, task);
      bridge.repositories = bridge.state.taskRecord?.repositories;
    } catch (error) {
      bridge.diagnostic = error.message;
      try {
        const binding = decide('task-binding', { state: readState(bindingFile(root, session), 16384) });
        if (binding?.task === task) bridge.repositories = binding.repositories;
      } catch { diagnostic('state', 'STATE_UNAVAILABLE'); }
    }
  }
  return { task, bridge };
}

export function locateCommand(root, options, request) {
  const { task, bridge } = observation(root, options.session, options.task);
  if (request !== undefined) bridge.request = request;
  const argv = Object.entries({ ...options, task, root }).filter(([, value]) => value !== undefined).flatMap(([key, value]) => (Array.isArray(value) ? value : [value]).flatMap(item => item === true ? [`--${key}`] : [`--${key}`, item]));
  const outcome = nativeCommand(['locate', ...argv], { telemetry: commandTelemetry(), discovery: bridge });
  forwardTelemetry(outcome.export);
  if (outcome.exitCode) throw Object.assign(new Error(outcome.stderr.trimEnd()), { code: outcome.errorCode });
  return JSON.parse(outcome.stdout);
}
