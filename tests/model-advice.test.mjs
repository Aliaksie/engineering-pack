import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { run } from '../cli/main.mjs';
import { validateProject } from '../application/project/configuration.mjs';

// model-advice runs in the native runtime; these tests use the public command line.
function modelAdvice(request, project = { formatVersion: 1 }) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-model-advice-'));
  try {
    fs.mkdirSync(path.join(root, '.craft'));
    fs.writeFileSync(path.join(root, '.craft/project.json'), JSON.stringify(project));
    fs.writeFileSync(path.join(root, 'request.json'), JSON.stringify(request));
    return run(['model-advice', '--root', root, '--data', path.join(root, 'request.json'), '--preferences', path.join(root, 'absent.json')]);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
}

const config = () => ({ formatVersion: 1, execution: {
  roles: { verifier: { mode: 'delegate', profile: 'bounded' } },
  profiles: { bounded: { capabilities: ['test-design'], maxAttempts: 1, escalateTo: 'deep' }, deep: { capabilities: ['test-design', 'cross-system'], maxAttempts: 2 } },
  hosts: { codex: { delegation: true, models: { bounded: 'consumer-model-a', deep: 'consumer-model-b' } } }
} });
const request = (extra = {}) => ({ host: 'codex', role: 'verifier', task: 'T1', revision: 'r1', requiredCapabilities: ['test-design'], ...extra });
const attempt = (extra = {}) => ({ id: 'a1', profile: 'bounded', task: 'T1', revision: 'r1', outcome: 'quality-failure', evidence: 'artifacts/acceptance-a1.json', ...extra });

test('model advice selects declared capabilities through existing host/role routing without execution', () => {
  const project = config();
  const selected = modelAdvice(request(), project);
  assert.equal(selected.decision, 'recommend');
  assert.equal(selected.execution.model, 'consumer-model-a');
  assert.equal(selected.execution.role, 'verifier');
  const capable = modelAdvice(request({ requiredCapabilities: ['cross-system'] }), project);
  assert.equal(capable.profile, 'deep');
  assert.equal(capable.execution.model, 'consumer-model-b');
  assert.match(capable.reasons[0], /lacks declared/);
  assert.equal(modelAdvice(request({ host: 'unknown' }), project).execution.mode, 'main');
  assert.equal(modelAdvice(request({ host: 'unknown' }), project).mainModel.model, null);
  project.execution.hosts.codex.delegation = false;
  assert.equal(modelAdvice(request(), project).decision, 'recommend');
  assert.equal(modelAdvice(request(), project).execution.mode, 'main');
});

test('quality escalation needs current-target evidence and stops at configured limits or access blockers', () => {
  const escalated = modelAdvice(request({ attempts: [attempt()] }), config());
  assert.equal(escalated.profile, 'deep');
  assert.ok(escalated.reasons.some((reason) => reason.includes('artifacts/acceptance-a1.json')));
  const stale = modelAdvice(request({ attempts: [attempt({ revision: 'older' })] }), config());
  assert.equal(stale.profile, 'bounded');
  assert.equal(stale.ignoredAttempts, 1);
  for (const outcome of ['permission-denied', 'tool-unavailable']) {
    const blocked = modelAdvice(request({ attempts: [attempt({ outcome })] }), config());
    assert.equal(blocked.decision, 'stop');
    assert.equal(blocked.profile, 'bounded');
    assert.equal(modelAdvice(request({ attempts: [attempt({ outcome, profile: 'deep' })] }), config()).decision, 'stop');
  }
  const completed = modelAdvice(request({ attempts: [attempt({ outcome: 'passed' })] }), config());
  assert.equal(completed.decision, 'completed');
  const exhausted = modelAdvice(request({ attempts: [attempt(), attempt({ id: 'a2', profile: 'deep' }), attempt({ id: 'a3', profile: 'deep' })] }), config());
  assert.equal(exhausted.decision, 'stop');
  assert.match(exhausted.reasons.at(-1), /No further escalation/);
  const missing = config(); delete missing.execution.hosts.codex.models.deep;
  assert.equal(modelAdvice(request({ requiredCapabilities: ['cross-system'] }), missing).decision, 'recommend');
  assert.equal(modelAdvice(request({ requiredCapabilities: ['cross-system'] }), missing).mainModel.model, null);
});

test('invalid profile graphs, unbounded attempts and uncited or duplicate failures are rejected', () => {
  const cycle = config(); cycle.execution.profiles.deep.escalateTo = 'bounded';
  assert.throws(() => validateProject(cycle), /cycle/);
  const unknown = config(); unknown.execution.profiles.deep.escalateTo = 'absent';
  assert.throws(() => validateProject(unknown), /unknown escalation/);
  const excessive = config(); excessive.execution.profiles.bounded.maxAttempts = 99;
  assert.throws(() => validateProject(excessive), /maxAttempts/);
  assert.throws(() => modelAdvice(request({ attempts: [attempt({ evidence: '' })] }), config()), /evidence/);
  assert.throws(() => modelAdvice(request({ attempts: [attempt(), attempt()] }), config()), /unique/);
  const undeclared = config(); delete undeclared.execution.profiles;
  assert.equal(modelAdvice(request(), undeclared).decision, 'unavailable');
});

test('inherited object properties never satisfy explicit profile or exact model mappings', () => {
  const project = { formatVersion: 1, execution: {
    roles: { verifier: { mode: 'delegate', profile: 'constructor' } },
    profiles: { constructor: { class: 'standard', capabilities: [] } },
    hosts: { codex: { delegation: true, mainModelSwitch: true, models: {} } }
  } };
  const advice = modelAdvice(request({ requiredCapabilities: [] }), project);
  assert.equal(advice.mainModel.model, null);
  assert.equal(advice.mainModel.switchSupported, false);
  assert.equal(advice.execution.mode, 'main');
  project.execution.profiles.undefined = { class: 'standard', capabilities: [] };
  assert.equal(modelAdvice(request({ requiredCapabilities: [] }), project).mainModel.decision, 'retain-current-until-authorized');
  delete project.execution.profiles;
  assert.equal(modelAdvice(request({ requiredCapabilities: [] }), project).decision, 'unavailable');
});

test('logical defaults give useful main-model advice without workers or exact model aliases', () => {
  const project = { formatVersion: 1, efficiency: { delegation: { maxWorkers: 0 } } };
  const advice = modelAdvice({ host: 'unknown', role: 'implementer', task: 'T1', revision: 'r1', capabilityClass: 'strong' }, project);
  assert.equal(advice.profile, 'strong');
  assert.equal(advice.execution.mode, 'main');
  assert.equal(advice.mainModel.model, null);
  assert.equal(advice.mainModel.switchSupported, false);
  assert.equal(advice.attemptsRemaining, 3);
  const adequate = modelAdvice({ host: 'unknown', role: 'implementer', task: 'T1', revision: 'r1', currentProfile: 'strong' }, project);
  assert.equal(adequate.mainModel.decision, 'keep-current');
  const mapped = { formatVersion: 1, execution: { hosts: { fixture: { mainModelSwitch: true, models: { standard: 'explicit-consumer-model' } } } } };
  const suggestion = modelAdvice({ host: 'fixture', role: 'implementer', task: 'T1', revision: 'r1' }, mapped);
  assert.equal(suggestion.mainModel.switchSupported, true);
  assert.equal(suggestion.mainModel.model, 'explicit-consumer-model');
  assert.equal(suggestion.execution.mode, 'main');
});

test('requested capability classes escalate through the configured chain', () => {
  const project = { formatVersion: 1, execution: {
    roles: { planner: { mode: 'delegate', profile: 'small' } },
    profiles: { small: { capabilities: [], class: 'economical', escalateTo: 'deep' }, deep: { capabilities: [], class: 'strong' } },
    hosts: { fixture: { delegation: true, models: { small: 'consumer-a', deep: 'consumer-b' } } }
  } };
  const result = modelAdvice({ host: 'fixture', role: 'planner', task: 'T1', revision: 'r1', capabilityClass: 'strong' }, project);
  assert.equal(result.profile, 'deep');
  assert.equal(result.attemptsRemaining, 3);
});

test('model advice requires a request file and reports invalid requests as usage errors', () => {
  assert.throws(() => run(['model-advice', '--root', os.tmpdir()]), /requires --data/);
  assert.throws(() => modelAdvice({ host: 'codex', role: 'writer', task: 'T1', revision: 'r1' }), /role/);
});
