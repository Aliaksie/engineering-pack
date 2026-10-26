import { stateFile } from '../platform/index.mjs';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { spawnSync } from 'node:child_process';
import test, { after } from 'node:test';
import assert from 'node:assert/strict';
import { attemptStatus } from './session-state.mjs';
import { readSessionState } from './session-state.mjs';
import { sessionStatus } from './session-cli.mjs';
import { decide } from '../application/engine-client.mjs';
import { catalog } from '../application/routing/catalog.mjs';
import { saveCheckpoint, recoverCheckpoint } from './checkpoint-cli.mjs';
import { run } from './craft.mjs';

// Discovery through the public locate command; `request` is its --data document.
let requestCount = 0;
function discover(root, request, { session, task, preferencesFile } = {}) {
  const data = path.join(requests, `${requestCount++}.json`);
  fs.writeFileSync(data, JSON.stringify(request));
  const scope = Object.entries({ session, task, preferences: preferencesFile }).filter(([, value]) => value !== undefined).flatMap(([key, value]) => [`--${key}`, value]);
  return run(['locate', '--root', root, ...scope, '--data', data]);
}

// The CLI reaches session advice through sessionStatus; these cases pin the decision itself.
const sessionAdvice = (input, project, options = {}) => decide('session-advice', { input, project, catalog, options: { now: Date.now(), ...options } });

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'session-efficiency-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const git = (...args) => {
    const result = spawnSync('git', ['-C', root, '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test', '-c', 'commit.gpgsign=false', ...args], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr); return result.stdout.trim();
  };
  const write = (name, content) => { fs.mkdirSync(path.dirname(path.join(root, name)), { recursive: true }); fs.writeFileSync(path.join(root, name), content); };
  git('init', '-q');
  write('src/names.txt', Array.from({ length: 8 }, (_, i) => `Symbol${i}`).join('\n'));
  write('package.json', '{}'); git('add', '.'); git('commit', '-qm', 'fixture');
  return { root, git, write };
}
const scope = { session: 's1', task: 'T1' };
// Attempts are recorded through the public CLI; the request file stays outside the worktree
// so it never changes the source the receipt is bound to.
const requests = fs.mkdtempSync(path.join(os.tmpdir(), 'session-attempt-'));
after(() => fs.rmSync(requests, { recursive: true, force: true }));
function recordAttempt(root, session, task, attempt) {
  const file = path.join(requests, 'attempt.json');
  fs.writeFileSync(file, JSON.stringify(attempt));
  return run(['session-attempt', '--root', root, '--session', session, '--task', task, '--data', file]);
}
const continuation = () => Object.fromEntries(['goal', 'currentState', 'completed', 'approach', 'evidence', 'filesChanged', 'validation', 'remainingWork', 'blockers', 'nextStep'].map(key => [key, `${key}: concise resume evidence`]));
const receipt = extra => ({ operation: 'implement', target: 'acceptance:regression', approach: 'bounded-fix', evidence: 'artifacts/check-1.json', outcome: 'quality-failure', ...extra });
const now = Date.now();
const observed = tokens => ({ sessionId: 's1', host: { name: 'fixture', reliableCompaction: true }, usage: {
  kind: 'current-context', currentTokens: tokens, capacityTokens: 100, source: 'fixture-current-context', observedAt: new Date(now).toISOString(), sessionId: 's1'
} });

test('known eight lookups use one bounded batch; five cached items leave exactly three fresh', t => {
  const { root } = fixture(t);
  const terms = Array.from({ length: 8 }, (_, i) => `Symbol${i}`);
  const initial = discover(root, { terms: terms.slice(0, 5), paths: ['src'] }, scope);
  assert.equal(initial.discovery.fresh, 5);
  const next = discover(root, { terms, paths: ['src'] }, scope);
  assert.deepEqual(next.discovery, { requested: 8, reused: 5, fresh: 3, batches: 1 });
  assert.equal(next.matches.length, 8);
  const repeated = discover(root, { terms, paths: ['src'] }, scope);
  assert.equal(repeated.filesScanned, 0);
  assert.equal(repeated.bytesRead, 0);
  assert.equal(repeated.discovery.reused, 8);
  const activity = readSessionState(root, scope.session, scope.task).activity;
  assert.equal(activity.reuseHits, 13);
  assert.equal(activity.batchedOperations, 2);
});

test('file, inventory, HEAD, config, manifest and task changes invalidate literal evidence', t => {
  const { root, git, write } = fixture(t);
  const request = { terms: ['Symbol0'], paths: ['src'] };
  const prime = () => { discover(root, request, scope); assert.equal(discover(root, request, scope).discovery.reused, 1); };
  prime(); write('src/names.txt', 'Symbol0 changed\n'); assert.equal(discover(root, request, scope).discovery.fresh, 1);
  prime(); write('src/new.txt', 'Symbol0 added\n'); assert.equal(discover(root, request, scope).discovery.fresh, 1);
  prime(); write('package.json', '{"type":"module"}'); assert.equal(discover(root, request, scope).discovery.fresh, 1);
  prime(); write('.craft/project.json', '{"formatVersion":1}'); assert.equal(discover(root, request, scope).discovery.fresh, 1);
  prime(); git('commit', '--allow-empty', '-qm', 'new revision'); assert.equal(discover(root, request, scope).discovery.fresh, 1);
  prime(); assert.equal(discover(root, request, { ...scope, task: 'T2' }).discovery.fresh, 1);
  assert.equal(discover(root, request, { ...scope, session: 's2' }).discovery.fresh, 1);
});

test('file-scoped lookup reuses evidence and invalidates on file and ancestor manifest changes', t => {
  const { root, write } = fixture(t);
  for (const file of ['src/names.txt', 'root.txt']) {
    write(file, 'FileScopedSymbol\n');
    const request = { terms: ['FileScopedSymbol'], paths: [file] };
    const prime = () => {
      const first = discover(root, request, scope);
      assert.equal(first.discovery.fresh, 1);
      assert.equal(first.reuse.validated, true);
      const repeated = discover(root, request, scope);
      assert.equal(repeated.discovery.reused, 1);
      assert.equal(repeated.filesScanned, 0);
      assert.equal(repeated.bytesRead, 0);
    };
    prime();
    write(file, 'FileScopedSymbol changed\n');
    prime();
    write('package.json', JSON.stringify({ changed: file }));
    prime();
    if (file.startsWith('src/')) {
      write('src/pyproject.toml', '[project]\nname = "fixture"\n');
      prime();
    }
    fs.unlinkSync(path.join(root, file));
    assert.throws(() => discover(root, request, scope), { code: 'ENOENT' }, 'a deleted scope must not return cached evidence');
  }
});

test('dependent lookup stays staged; arbitrary actions and side effects cannot enter a lookup batch', t => {
  const { root } = fixture(t);
  const result = discover(root, { requests: [{ id: 'framework', term: 'Symbol0' }, { id: 'guidance', term: 'Symbol1', dependsOn: ['framework'] }], paths: ['src'] });
  assert.equal(result.discovery.fresh, 1);
  assert.equal(result.deferred[0].id, 'guidance');
  assert.deepEqual(result.matches.map(match => match.text), ['Symbol0']);
  assert.throws(() => discover(root, { requests: [{ id: 'write', term: 'Symbol0', command: 'deploy' }] }), /side effects/);
  assert.throws(() => discover(root, { requests: [{ id: 'write', term: 'Symbol0', dependsOn: ['missing'] }] }), /dependency/);
});

test('large evidence remains within the output cap and truncated packets never imply absence', t => {
  const { root, write } = fixture(t);
  write('src/big.txt', Array.from({ length: 300 }, () => `needle ${'x'.repeat(220)}`).join('\n'));
  const result = discover(root, { terms: ['needle'], paths: ['src'], maxMatches: 1000, maxOutputBytes: 4096 }, scope);
  assert.equal(result.truncated, true);
  assert.ok(result.truncation.includes('maxOutputBytes'));
  assert.ok(Buffer.byteLength(JSON.stringify(result)) <= 4096);
  assert.ok(result.matches.every(match => match.text.length <= 240));
  assert.match(result.limitations.join(' '), /not proof of absence/);
});

test('three equivalent failures stop, a changed approach is distinguished, and pass ends the attempt series', t => {
  const { root, write } = fixture(t);
  for (let i = 0; i < 3; i++) recordAttempt(root, scope.session, scope.task, receipt());
  let state = readSessionState(root, scope.session, scope.task);
  assert.equal(attemptStatus(state.attempts, 'implement').noProgressAttempts, 3);
  let status = sessionStatus({ task: { id: scope.task, stage: 'implement' } }, undefined, { root, session: scope.session });
  assert.equal(status.recommendation.action, 'return-control');
  recordAttempt(root, scope.session, scope.task, receipt({ approach: 'different-evidence', maxAttempts: 5 }));
  state = readSessionState(root, scope.session, scope.task);
  assert.equal(attemptStatus(state.attempts, 'implement').noProgressAttempts, 1);
  recordAttempt(root, scope.session, scope.task, receipt({ outcome: 'passed' }));
  assert.equal(attemptStatus(readSessionState(root, scope.session, scope.task).attempts, 'implement').attempts, 0);
  write('src/names.txt', 'progress\n');
  recordAttempt(root, scope.session, scope.task, receipt());
  assert.equal(attemptStatus(readSessionState(root, scope.session, scope.task).attempts, 'implement').noProgressAttempts, 1);
  assert.throws(() => recordAttempt(root, scope.session, scope.task, { outcome: 'quality-failure' }), /meaningful attempt/);
});

test('session aggregation verifies continuation freshness, selects guidance, and resumes with only next-step evidence', t => {
  const { root, write } = fixture(t);
  const input = { ...observed(79), task: { id: scope.task, stage: 'implement', safeBoundary: true, handoffSaved: true, checkpointFresh: true } };
  let status = sessionStatus(input, undefined, { root, now });
  assert.equal(status.recommendation.action, 'save-handoff', 'caller freshness cannot bypass absent checkpoint');
  saveCheckpoint(root, scope.session, { taskId: scope.task, continuation: continuation() });
  status = sessionStatus(input, undefined, { root, now });
  assert.equal(status.checkpoint.status, 'fresh');
  assert.equal(status.recommendation.action, 'compact');
  assert.ok(status.packContext.estimatedTokens > 0);
  assert.ok(status.packReport.files.every(file => file.sha256));
  assert.equal(status.checkpoint.nextStep, continuation().nextStep);
  assert.equal(sessionStatus({ ...input, task: { ...input.task, id: 'other-task' } }, undefined, { root, now }).checkpoint.taskMatches, false);
  write('src/names.txt', 'changed\n');
  assert.equal(sessionStatus(input, undefined, { root, now }).checkpoint.status, 'stale-or-incomplete');
  assert.equal(recoverCheckpoint(root, scope.session).changed, true);
});

test('policy distinguishes healthy progress, unknown telemetry, pressure and changed phase', t => {
  const { root } = fixture(t);
  const task = { id: scope.task, stage: 'implement', status: 'working' };
  assert.equal(sessionStatus({ ...observed(45), task }, undefined, { root, now }).recommendation, null);
  assert.equal(sessionStatus({ task }, undefined, { root, now }).usage.status, 'unknown');
  assert.equal(sessionStatus({ ...observed(91), task: { ...task, safeBoundary: true, phaseChanged: true } }, undefined, { root, now }).recommendation.action, 'save-handoff');
  assert.equal(sessionAdvice({ ...observed(91), task }, undefined, { now }).recommendation.action, 'continue-bounded-step');
});

test('classes use deterministic economical discovery and allow directly requested strong reasoning', () => {
  const capabilityAdvice = (operation, risks) => decide('capability', { operation, risks });
  assert.equal(capabilityAdvice('locate').class, 'economical');
  assert.equal(capabilityAdvice('locate').execution, 'prefer-deterministic-tool');
  assert.equal(capabilityAdvice('architecture').class, 'strong');
  assert.equal(capabilityAdvice('implement', ['distributed-consistency']).class, 'strong');
  assert.equal(capabilityAdvice('implement').class, 'standard');
});

test('session receipts remain opt-in while normal commands record only supporting telemetry', t => {
  const { root } = fixture(t);
  run(['route', '--root', root, '--intent', 'locate']);
  assert.ok(fs.readdirSync(path.dirname(stateFile(root, 'probe.json'))).every(name => name.startsWith('telemetry')));
  assert.equal(readSessionState(root, scope.session, scope.task).activity.routingDecisions, undefined);
  run(['route', '--root', root, '--intent', 'locate', '--session', scope.session, '--task', scope.task]);
  run(['locate', '--root', root, '--term', 'Symbol0', '--path', 'src', '--session', scope.session, '--task', scope.task]);
  const status = run(['session', '--root', root, '--session', scope.session, '--task', scope.task]);
  assert.equal(status.activity.routingDecisions, 1);
  assert.equal(status.discovery.requested, 1);
  assert.equal(status.usage.status, 'unknown');
});

test('configured quality escalation is offered, access failures stop, and escalation receipts retain reasons', t => {
  const { root, git, write } = fixture(t);
  const project = { formatVersion: 1, execution: {
    roles: { implementer: { mode: 'delegate', profile: 'routine' } },
    profiles: { routine: { capabilities: [], class: 'standard', maxAttempts: 3, escalateTo: 'deep' }, deep: { capabilities: [], class: 'strong' } },
    hosts: { fixture: { delegation: true, models: { routine: 'consumer-a', deep: 'consumer-b' } } }
  } };
  write('.craft/project.json', JSON.stringify(project));
  const revision = git('rev-parse', 'HEAD');
  const input = { sessionId: scope.session, host: { name: 'fixture' }, task: { id: scope.task, stage: 'implement', revision } };
  for (let index = 0; index < 3; index++) recordAttempt(root, scope.session, scope.task, receipt({ profile: 'routine' }));
  let status = sessionStatus(input, project, { root });
  assert.equal(status.recommendation.action, 'escalate-model');
  assert.equal(status.modelAdvice.profile, 'deep');
  assert.match(status.recommendation.reason, /quality-attempt limit/);
  recordAttempt(root, scope.session, scope.task, receipt({ profile: 'deep', escalationReason: 'routine quality budget exhausted', outcome: 'tool-unavailable' }));
  status = sessionStatus(input, project, { root });
  assert.equal(status.recommendation.action, 'stop-blocked');
  assert.equal(status.modelAdvice.decision, 'stop');
  assert.equal(status.activity.modelEscalations, 1);
  assert.equal(readSessionState(root, scope.session, scope.task).attempts.at(-1).escalationReason, 'routine quality budget exhausted');
});

test('corrupt or symlinked activity state cannot validate reuse or permit context reset', t => {
  const { root } = fixture(t);
  discover(root, { terms: ['Symbol0'], paths: ['src'] }, scope);
  const directory = path.dirname(stateFile(root, 'probe.json', false, scope.session));
  const file = fs.readdirSync(directory).find(name => name.startsWith('session-'));
  fs.writeFileSync(path.join(directory, file), '{broken');
  const result = discover(root, { terms: ['Symbol0'], paths: ['src'] }, scope);
  assert.equal(result.discovery.reused, 0);
  assert.equal(result.discovery.fresh, 1);
  assert.ok(result.reuse.diagnostic);
  const status = sessionStatus({ ...observed(91), task: { id: scope.task, stage: 'implement', safeBoundary: true, phaseChanged: true, handoffSaved: true, checkpointFresh: true } }, undefined, { root, now });
  assert.equal(status.recommendation.action, 'save-handoff');
  assert.ok(status.diagnostics.some(item => item.includes('Activity unavailable')));
});

test('normalized scopes share safe reuse and edits still invalidate dot-prefixed scopes', t => {
  const { root, write } = fixture(t);
  discover(root, { terms: ['Symbol0'], paths: ['./src/'] }, scope);
  assert.equal(discover(root, { terms: ['Symbol0'], paths: ['src'] }, scope).discovery.reused, 1);
  write('src/names.txt', 'Symbol0 changed\n');
  assert.equal(discover(root, { terms: ['Symbol0'], paths: ['./src/'] }, scope).discovery.fresh, 1);
});

test('three identical rediscovery requests without new evidence surface a loop without counting messages', t => {
  const { root } = fixture(t);
  for (let index = 0; index < 4; index++) discover(root, { terms: ['Symbol0'], paths: ['src'] }, scope);
  const status = sessionStatus({ task: { id: scope.task, stage: 'locate' } }, undefined, { root, session: scope.session });
  assert.equal(status.recommendation.action, 'return-control');
  assert.equal(status.activity.noProgressAttempts, 3);
  assert.equal(status.activity.operationsAttempted, null);
});

test('incomplete repository inspection records budgets while leaving equivalence unknown', t => {
  const { root, write } = fixture(t);
  write('oversized.txt', 'x'.repeat(2 * 1024 * 1024 + 1));
  for (let index = 0; index < 3; index++) recordAttempt(root, scope.session, scope.task, receipt());
  const status = attemptStatus(readSessionState(root, scope.session, scope.task).attempts, 'implement');
  assert.equal(status.attempts, 3);
  assert.equal(status.noProgressAttempts, 0);
  assert.equal(status.records.at(-1).source, null);
});

test('attempt operations are bounded identifiers and help avoids source-code rediscovery', t => {
  const { root } = fixture(t);
  recordAttempt(root, scope.session, scope.task, receipt({ operation: 'acceptance', outcome: 'passed' }));
  const status = sessionStatus({ task: { id: scope.task, stage: 'verify', operation: 'acceptance' } }, undefined, { root, session: scope.session });
  assert.equal(status.operation, 'acceptance');
  assert.equal(status.activity.attempts, 0);
  assert.equal(run(['session-attempt', '--help']).example.outcome, 'passed');
  assert.match(run(['checkpoint-read', '--help']).usage, /--session id/);
  assert.ok(run(['--help']).commands.includes('locate'));
});

test('known host mappings identify an unnecessarily strong profile for mechanical lookup without switching it', t => {
  const { root } = fixture(t);
  const project = { formatVersion: 1, execution: {
    roles: { locator: { mode: 'delegate', profile: 'small' } },
    profiles: { small: { class: 'economical', capabilities: [] }, deep: { class: 'strong', capabilities: [] } },
    hosts: { fixture: { delegation: true, models: { small: 'consumer-a', deep: 'consumer-b' } } }
  } };
  fs.mkdirSync(path.join(root, '.craft'), { recursive: true });
  fs.writeFileSync(path.join(root, '.craft/project.json'), JSON.stringify(project));
  const status = sessionStatus({ host: { name: 'fixture', model: 'consumer-b' }, task: { id: scope.task, stage: 'locate', revision: 'r1' } }, project, { root });
  assert.equal(status.capability.currentClass, 'strong');
  assert.equal(status.capability.recommendation, 'use-deterministic-tool-or-economical-profile');
  assert.equal(status.modelAdvice.profile, 'small');
  assert.equal(status.host.model, 'consumer-b');
});

test('unavailable optional activity storage does not suppress the routing plan', t => {
  const { root, write } = fixture(t);
  const directory = path.dirname(stateFile(root, 'probe.json', false, scope.session));
  fs.mkdirSync(path.dirname(directory), { recursive: true });
  fs.writeFileSync(directory, 'not a directory');
  const result = run(['route', '--root', root, '--intent', 'locate', '--session', scope.session, '--task', scope.task]);
  assert.equal(result.intent, 'locate');
  assert.ok(result.activityDiagnostic);
});

test('empty discovery remains invalid and healthy reuse alone does not imply post-compaction degradation', () => {
  assert.throws(() => discover('.', { terms: [] }), /terms/);
  const status = sessionAdvice({ ...observed(91), task: { safeBoundary: true, handoffSaved: true, checkpointFresh: true, compacted: true }, activity: { repeatedLookups: 8, reuseHits: 8 } }, undefined, { now });
  assert.equal(status.recommendation.action, 'compact');
});

test('escalation starts the new tier budget and configured terminal limits stop before the generic ceiling', t => {
  const { root, git, write } = fixture(t);
  const project = { formatVersion: 1, execution: {
    roles: { implementer: { mode: 'delegate', profile: 'routine' } },
    profiles: { routine: { capabilities: [], class: 'standard', maxAttempts: 1, escalateTo: 'deep' }, deep: { capabilities: [], class: 'strong', maxAttempts: 2 } },
    hosts: { fixture: { delegation: true, models: { routine: 'consumer-a', deep: 'consumer-b' } } }
  } };
  write('.craft/project.json', JSON.stringify(project));
  const input = { sessionId: scope.session, host: { name: 'fixture' }, task: { id: scope.task, stage: 'implement', revision: git('rev-parse', 'HEAD') } };
  recordAttempt(root, scope.session, scope.task, receipt({ profile: 'routine' }));
  assert.equal(sessionStatus(input, project, { root }).recommendation.action, 'escalate-model');
  recordAttempt(root, scope.session, scope.task, receipt({ profile: 'deep' }));
  let status = sessionStatus(input, project, { root });
  assert.equal(status.activity.attempts, 1);
  assert.equal(status.recommendation.action, 'retry-differently');
  recordAttempt(root, scope.session, scope.task, receipt({ profile: 'deep' }));
  status = sessionStatus(input, project, { root });
  assert.equal(status.activity.attempts, 2);
  assert.equal(status.recommendation.action, 'return-control');
  assert.match(status.recommendation.reason, /No further escalation/);
});

test('switching profiles cannot clear an unresolved permission failure', () => {
  const records = [
    { operation: 'implement', profile: 'routine', outcome: 'permission-denied', evidence: 'denial' },
    { operation: 'implement', profile: 'deep', outcome: 'quality-failure', evidence: 'check' }
  ];
  assert.equal(attemptStatus(records, 'implement').attempts, 1);
  assert.equal(attemptStatus(records, 'implement').blocked, true);
  assert.equal(attemptStatus([...records, { operation: 'implement', profile: 'deep', outcome: 'passed' }], 'implement').blocked, false);
});

test('a mechanical operation within implementation receives economical capability advice', t => {
  const { root } = fixture(t);
  const status = sessionStatus({ task: { stage: 'implement', operation: 'metadata' } }, undefined, { root });
  assert.equal(status.operation, 'metadata');
  assert.equal(status.capability.class, 'economical');
});

test('automatic repository snapshots cannot renew a stale semantic continuation', t => {
  const { root, write } = fixture(t);
  const input = { ...observed(79), task: { id: scope.task, stage: 'implement', safeBoundary: true } };
  const original = saveCheckpoint(root, scope.session, { taskId: scope.task, continuation: continuation() }).state;
  assert.equal(sessionStatus(input, undefined, { root, now }).recommendation.action, 'compact');
  write('src/names.txt', 'new work since handoff\n');
  const automatic = saveCheckpoint(root, scope.session).state;
  assert.deepEqual(automatic.continuationSource, original.continuationSource);
  assert.equal(recoverCheckpoint(root, scope.session).changed, false, 'the automatic Git snapshot itself is current');
  const stale = sessionStatus(input, undefined, { root, now });
  assert.equal(stale.checkpoint.status, 'stale-or-incomplete');
  assert.equal(stale.recommendation.action, 'save-handoff', 'old continuation source cannot authorize context replacement');
  saveCheckpoint(root, scope.session, { taskId: scope.task, continuation: { ...continuation(), nextStep: 'Resume from the newly validated source state.' } });
  assert.equal(sessionStatus(input, undefined, { root, now }).recommendation.action, 'compact');
});

function retryProject() {
  return { formatVersion: 1, execution: {
    roles: { implementer: { mode: 'delegate', profile: 'routine' } },
    profiles: { routine: { capabilities: [], class: 'standard', maxAttempts: 3, escalateTo: 'deep' }, deep: { capabilities: [], class: 'strong', maxAttempts: 2 } },
    hosts: { fixture: { delegation: true, models: { routine: 'consumer-a', deep: 'consumer-b' } } }
  } };
}

test('a completed series cannot suppress escalation or retain a resolved denial in a later series', t => {
  const { root, git } = fixture(t);
  const project = retryProject();
  fs.mkdirSync(path.join(root, ".craft"), { recursive: true });
  fs.writeFileSync(path.join(root, ".craft/project.json"), JSON.stringify(project));
  const input = { sessionId: scope.session, host: { name: 'fixture' }, task: { id: scope.task, stage: 'implement', revision: git('rev-parse', 'HEAD') } };
  recordAttempt(root, scope.session, scope.task, receipt({ profile: 'routine', outcome: 'permission-denied' }));
  recordAttempt(root, scope.session, scope.task, receipt({ profile: 'routine', outcome: 'passed' }));
  assert.equal(sessionStatus(input, project, { root }).modelAdvice.decision, 'completed');
  recordAttempt(root, scope.session, scope.task, receipt({ profile: 'routine' }));
  let status = sessionStatus(input, project, { root });
  assert.equal(status.activity.attempts, 1);
  assert.equal(status.recommendation.action, 'retry-differently');
  assert.equal(status.modelAdvice.decision, 'recommend');
  for (let i = 0; i < 2; i++) recordAttempt(root, scope.session, scope.task, receipt({ profile: 'routine' }));
  status = sessionStatus(input, project, { root });
  assert.equal(status.activity.attempts, 3);
  assert.equal(status.recommendation.action, 'escalate-model');
  assert.equal(status.modelAdvice.profile, 'deep');
});

test('unrelated completed operations cannot evict active blockers or escalation evidence', t => {
  const { root, git } = fixture(t);
  const project = retryProject();
  fs.mkdirSync(path.join(root, ".craft"), { recursive: true });
  fs.writeFileSync(path.join(root, ".craft/project.json"), JSON.stringify(project));
  const input = { sessionId: scope.session, host: { name: 'fixture' }, task: { id: scope.task, stage: 'implement', revision: git('rev-parse', 'HEAD') } };
  const unrelated = () => {
    for (let i = 0; i < 24; i++) recordAttempt(root, scope.session, scope.task, receipt({ operation: 'verify', target: `check:${i}`, outcome: 'passed' }));
    assert.equal(readSessionState(root, scope.session, scope.task).attempts.length, 24);
  };
  recordAttempt(root, scope.session, scope.task, receipt({ profile: 'routine', outcome: 'permission-denied' }));
  unrelated();
  assert.equal(sessionStatus(input, project, { root }).recommendation.action, 'stop-blocked');
  recordAttempt(root, scope.session, scope.task, receipt({ profile: 'deep' }));
  assert.equal(sessionStatus(input, project, { root }).recommendation.action, 'stop-blocked');
  recordAttempt(root, scope.session, scope.task, receipt({ profile: 'deep', outcome: 'passed' }));
  for (let i = 0; i < 3; i++) recordAttempt(root, scope.session, scope.task, receipt({ profile: 'routine' }));
  unrelated();
  const status = sessionStatus(input, project, { root });
  assert.equal(status.activity.attempts, 3);
  assert.equal(status.activity.noProgressAttempts, 3);
  assert.equal(status.recommendation.action, 'escalate-model');
  assert.equal(status.modelAdvice.profile, 'deep');
});

test('active receipt capacity fails explicitly without changing persisted evidence and success releases it', t => {
  const { root } = fixture(t);
  for (let i = 0; i < 24; i++) recordAttempt(root, scope.session, scope.task, receipt({ operation: `operation:${i}` }));
  const before = readSessionState(root, scope.session, scope.task);
  assert.throws(() => recordAttempt(root, scope.session, scope.task, receipt()), /attempt capacity reached/);
  assert.deepEqual(readSessionState(root, scope.session, scope.task), before);
  recordAttempt(root, scope.session, scope.task, receipt({ operation: 'operation:0', outcome: 'passed' }));
  recordAttempt(root, scope.session, scope.task, receipt());
  const after = readSessionState(root, scope.session, scope.task);
  assert.equal(after.attempts.length, 24);
  assert.equal(attemptStatus(after.attempts, 'implement').attempts, 1);
  assert.equal(attemptStatus(after.attempts, 'operation:1').attempts, 1);
});

