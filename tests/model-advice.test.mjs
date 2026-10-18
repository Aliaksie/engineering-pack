import test from 'node:test';
import assert from 'node:assert/strict';
import { modelAdvice } from '../application/session/models.mjs';
import { validateProject } from '../application/project/configuration.mjs';

const config = () => ({ formatVersion: 1, execution: {
  roles: { verifier: { mode: 'delegate', profile: 'bounded' } },
  profiles: { bounded: { capabilities: ['test-design'], maxAttempts: 1, escalateTo: 'deep' }, deep: { capabilities: ['test-design', 'cross-system'], maxAttempts: 2 } },
  hosts: { codex: { delegation: true, models: { bounded: 'consumer-model-a', deep: 'consumer-model-b' } } }
} });
const request = (extra = {}) => ({ host: 'codex', role: 'verifier', task: 'T1', revision: 'r1', requiredCapabilities: ['test-design'], ...extra });
const attempt = (extra = {}) => ({ id: 'a1', profile: 'bounded', task: 'T1', revision: 'r1', outcome: 'quality-failure', evidence: 'artifacts/acceptance-a1.json', ...extra });

test('model advice selects declared capabilities through existing host/role routing without execution', () => {
  const project = config(), original = structuredClone(project);
  const selected = modelAdvice(request(), project);
  assert.equal(selected.decision, 'recommend');
  assert.equal(selected.execution.model, 'consumer-model-a');
  assert.equal(selected.execution.role, 'verifier');
  assert.deepEqual(project, original);
  const capable = modelAdvice(request({ requiredCapabilities: ['cross-system'] }), project);
  assert.equal(capable.profile, 'deep');
  assert.equal(capable.execution.model, 'consumer-model-b');
  assert.match(capable.reasons[0], /lacks declared/);
  assert.equal(modelAdvice(request({ host: 'unknown' }), project).decision, 'unavailable');
  project.execution.hosts.codex.delegation = false;
  assert.equal(modelAdvice(request(), project).decision, 'unavailable');
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
  assert.equal(modelAdvice(request({ requiredCapabilities: ['cross-system'] }), missing).decision, 'unavailable');
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
