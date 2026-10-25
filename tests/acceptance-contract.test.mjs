import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { evaluate } from '../engine/index.mjs';

const fixture = name => JSON.parse(fs.readFileSync(new URL(`./fixtures/acceptance/${name}.json`, import.meta.url), 'utf8'));
// task-start runs in the native runtime; its frozen corpus holds these legacy starts.
const taskStarts = JSON.parse(fs.readFileSync(new URL('./fixtures/runtime/task-contracts.json', import.meta.url), 'utf8')).cases;
const started = id => taskStarts.find(entry => entry.id === `task-start:${id}`).response.result;
function call(operation, input) {
  const response = evaluate({ formatVersion: 1, operation, input });
  assert.equal(response.ok, true, JSON.stringify(response));
  return response.result;
}
const current = { kind: 'repository', identity: 'snapshot-a' };
function record(contract, overrides = {}) {
  return { id: 'check-value', criterionIds: ['value'], scope: ['src'],
    contract: call('engineering-contract-digest', { contract }), binding: current, checkId: 'value-check', kind: 'check',
    artifact: 'retained/value-output', observedAt: '2026-10-22T00:00:00Z', reporter: 'fixture runner',
    basis: 'Assert returned value equals the expected value', support: 'verified', execution: 'passed', ...overrides };
}
function coverage(contract, records, binding = current, blockers = false) {
  return call('acceptance-coverage', { contract, evidence: { formatVersion: 1, records }, current: binding, blockers });
}
function invalid(operation, input, reason) {
  const response = evaluate({ formatVersion: 1, operation, input });
  assert.equal(response.ok, false);
  assert.equal(response.error.code, 'INVALID_INPUT');
  if (reason) assert.equal(response.error.message, `acceptance:${reason}`);
}

test('portable contract round trips without mutating inputs and has key-order independent identity', () => {
  const contract = fixture('contract'), before = JSON.stringify(contract);
  assert.deepEqual(call('engineering-contract-validate', { contract }), contract);
  const reordered = Object.fromEntries(Object.entries(contract).reverse());
  assert.equal(call('engineering-contract-digest', { contract }), call('engineering-contract-digest', { contract: reordered }));
  assert.equal(JSON.stringify(contract), before);
  for (const mutate of [c => { c.criteria[0].description = 'Changed acceptance'; },
    c => { c.context.sources[0].identity = 'request-v2'; }, c => { c.context.sources[0].status = 'proposed'; }]) {
    const changed = structuredClone(contract); mutate(changed);
    assert.notEqual(call('engineering-contract-digest', { contract }), call('engineering-contract-digest', { contract: changed }));
  }
});

test('criterion A never covers B; optional gaps and unrelated checks stay visible', () => {
  const contract = fixture('contract'), value = record(contract);
  const partial = coverage(contract, [value]);
  assert.equal(partial.complete, false);
  assert.deepEqual(partial.criteria.map(entry => entry.status), ['verified', 'gap']);
  const error = record(contract, { id: 'check-error', criterionIds: ['error'] });
  assert.equal(coverage(contract, [value, error]).complete, true);
  assert.equal(coverage(contract, [value, error], current, true).complete, false);
  assert.equal(coverage(contract, [value, { ...error, checkId: 'irrelevant-build' }]).complete, false);
  assert.equal(coverage(contract, [value, { ...error, scope: ['elsewhere'] }]).complete, false);
  contract.criteria[1].required = false;
  const optional = coverage(contract, [record(contract)]);
  assert.equal(optional.complete, true);
  assert.equal(optional.criteria[1].status, 'gap');
});

test('stale, unknown, unexecuted and weak support cannot satisfy required criteria', () => {
  const contract = fixture('contract');
  for (const overrides of [{ execution: 'failed' }, { execution: 'not-run' }, { execution: 'not-available' },
    { support: 'observed' }, { support: 'inferred' }, { support: 'unknown' }, { kind: 'manual' },
    { binding: { ...current, identity: null } }, { contract: 'a'.repeat(64) }]) {
    const result = coverage(contract, [record(contract, { criterionIds: ['value', 'error'], ...overrides })]);
    assert.equal(result.complete, false, JSON.stringify(overrides));
  }
  const records = [record(contract, { criterionIds: ['value', 'error'] })];
  assert.equal(coverage(contract, records, { ...current, identity: 'snapshot-b' }).records[0].freshness, 'stale');
  assert.equal(coverage(contract, records, { ...current, identity: null }).records[0].freshness, 'unknown');
  contract.criteria[0].description = 'Amended contract without a source edit';
  assert.equal(coverage(contract, records).records[0].freshness, 'stale');
});

test('non-Git QA binds to an explicit environment and manual assessment remains attributed', () => {
  const contract = fixture('contract');
  contract.criteria.forEach(entry => { entry.evidenceKind = 'manual'; });
  const environment = { kind: 'environment', identity: 'build-42/browser-fixture' };
  const evidence = record(contract, { criterionIds: ['value', 'error'], kind: 'manual', binding: environment });
  const result = coverage(contract, [evidence], environment);
  assert.equal(result.complete, true);
  assert.equal(result.assurance, 'attributed');
  assert.equal(coverage(contract, [evidence], { ...environment, identity: 'build-43/browser-fixture' }).complete, false);
  assert.equal(coverage(contract, [evidence], { kind: 'repository', identity: environment.identity }).complete, false);
});

test('conflicting required constraints, open decisions and failures cannot be hidden by a pass', () => {
  const contract = fixture('contract');
  contract.context.constraints = ['allow', 'deny'].map((id, index) => ({ id, description: id,
    sourceIds: ['request'], authority: 'project', status: 'accepted', scope: ['src'], required: true,
    conflicts: [index ? 'allow' : 'deny'] }));
  const good = () => record(contract, { criterionIds: ['value', 'error'] });
  assert.deepEqual(coverage(contract, [good()]).conflicts, ['allow', 'deny']);
  assert.equal(coverage(contract, [good()]).complete, false);
  contract.context.constraints = [];
  contract.decisions = [{ id: 'decision', description: 'Unanswered product question', blocking: true, resolved: false }];
  assert.equal(coverage(contract, [good()]).complete, false);
  contract.decisions = [];
  assert.equal(coverage(contract, [good(), { ...good(), id: 'failed-check', execution: 'failed' }]).complete, false);
});

test('invalid, ambiguous, unsupported and oversized payloads fail explicitly', () => {
  for (const [mutate, reason] of [
    [c => { c.formatVersion = 2; }, 'unsupported-version'],
    [c => { c.authorization = 'execute arbitrary source commands'; }, 'invalid-fields'],
    [c => { c.criteria[1].id = c.criteria[0].id; }, 'duplicate-id'],
    [c => { c.criteria[0].sourceIds = ['missing']; }, 'unknown-reference'],
    [c => { c.criteria[0].sourceIds = []; }, 'missing-basis'],
    [c => { c.criteria = Array(33).fill(c.criteria[0]); }, 'capacity'],
    [c => { c.criteria[0].scope = ['C:\\source']; }, null],
    [c => { delete c.context.sources[0].identity; }, 'invalid-text'],
    [c => { c.exclusions = Array(32).fill('🙂'.repeat(256)); }, 'capacity']
  ]) {
    const contract = fixture('contract'); mutate(contract);
    invalid('engineering-contract-validate', { contract }, reason);
  }
  const contract = fixture('contract');
  invalid('acceptance-evidence-validate', { evidence: { formatVersion: 1, records: Array(65).fill(record(contract)) } }, 'capacity');
  invalid('acceptance-coverage', { contract, evidence: { formatVersion: 1, records: [record(contract, { criterionIds: ['missing'] })] }, current }, 'unknown-reference');
});

test('legacy lifecycle and checkpoint fixtures retain declared completion without inferred coverage', () => {
  const legacy = fixture('legacy');
  const stepped = call('task-step', { state: started('legacy-start').state, input: legacy.step, repository: legacy.repository, id: 'legacy-pass', at: legacy.at });
  const frozen = fixture('legacy-completed');
  assert.deepEqual(stepped.state, frozen.state);
  assert.equal(call('task-contract', { record: stepped.state.taskRecord }), frozen.checkpoint.taskContract);
  assert.deepEqual(call('checkpoint-validate', { state: frozen.checkpoint }), frozen.checkpoint);
  assert.equal(stepped.state.taskRecord.goalComplete, true);
  assert.equal(call('activity-validate', { state: stepped.state }).formatVersion, 1);
  const checkpoint = call('checkpoint-state', { previous: {}, metadata: { taskId: 'fix-value', continuation: stepped.continuation }, repository: legacy.repository, updated: legacy.at });
  assert.equal(call('checkpoint-validate', { state: checkpoint }).formatVersion, 1);
  assert.deepEqual(call('acceptance-coverage', { contract: null }), { assurance: 'declared', coverage: 'unknown', complete: false, criteria: [] });
  // Legacy declared checkpoints never acquire criterion coverage; receipts: attempt-contracts.json.
  invalid('checkpoint-metadata', { metadata: { engineeringContract: fixture('contract') } });
  invalid('activity-validate', { state: { ...legacy.state, formatVersion: 2 } });
  const augmented = { ...frozen.state, taskRecord: { ...frozen.state.taskRecord, engineeringContract: fixture('contract') } };
  invalid('activity-validate', { state: augmented });
  const upgraded = started('legacy-contract-upgrade');
  assert.equal(upgraded.state.formatVersion, 2);
  assert.equal(upgraded.state.taskRecord.goalComplete, false);
  assert.equal(call('activity-validate', { state: upgraded.state }).formatVersion, 2);
  const augmentedCheckpoint = { ...frozen.checkpoint, engineeringContract: fixture('contract') };
  assert.deepEqual(call('checkpoint-validate', { state: augmentedCheckpoint }), augmentedCheckpoint);
});
