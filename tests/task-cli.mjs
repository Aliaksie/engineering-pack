// task-step and the action commands run in the native runtime; tests drive them through the
// public CLI with the same inputs the removed JS entry points took.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { run } from '../cli/main.mjs';

function file(value) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-task-cli-'));
  const target = path.join(directory, 'input.json');
  fs.writeFileSync(target, JSON.stringify(value));
  return target;
}
const flag = (name, value) => value === undefined ? [] : [`--${name}`, String(value)];

export function stepTask(root, session, task, input, { host, preferencesFile, bundle } = {}) {
  return run(['task-step', '--root', root, '--session', session, '--task', task,
    ...flag('stage', input.stage), ...flag('target', input.target), ...flag('approach', input.approach), ...flag('evidence', input.evidence),
    ...flag('outcome', input.outcome), ...flag('result', input.result), ...flag('next', input.nextStep), ...flag('profile', input.profile),
    ...(input.acceptanceEvidence !== undefined ? ['--acceptance-evidence', file(input.acceptanceEvidence),
      ...flag('expected-task-contract', input.expectedTaskContract), ...flag('expected-evidence-digest', input.expectedEvidenceDigest)] : []),
    ...(input.complete ? ['--complete'] : []), ...flag('host', host), ...flag('preferences', preferencesFile),
    ...[bundle ?? []].flat().flatMap(root => ['--bundle', root])]);
}

export function taskAction(root, session, task, operation, input, facts = {}, { preferencesFile, bundle } = {}) {
  const common = ['--root', root, '--session', session, '--task', task, ...flag('preferences', preferencesFile),
    ...[bundle ?? []].flat().flatMap(root => ['--bundle', root]), ...(Object.keys(facts).length ? ['--data', file(facts)] : [])];
  if (operation === 'action-propose') return run(['task-offer', ...common, ...flag('action', input.action), ...flag('scope', input.scope),
    ...flag('benefit', input.benefit), ...flag('evidence', input.evidence), ...flag('role', input.role), ...flag('profile', input.profile),
    ...flag('prior-authorization', input.priorAuthorization)]);
  if (operation === 'action-decide') return run(['task-decision', ...common, ...flag('offer', input.id), ...flag('decision', input.decision), ...flag('evidence', input.evidence)]);
  if (operation === 'action-check') return run(['task-action-check', ...common, ...flag('offer', input.id)]);
  throw new Error(`unknown action operation: ${operation}`);
}
