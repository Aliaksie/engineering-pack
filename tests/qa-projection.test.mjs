import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { evaluate } from '../engine/index.mjs';
import { engineeringContractDigest } from '../engine/session/acceptance.mjs';
import { resetApplication } from '../examples/evals/manual-qa/fixture/reset.mjs';

const load = name => JSON.parse(fs.readFileSync(new URL(`../examples/evals/manual-qa/${name}.json`, import.meta.url), 'utf8'));
function fixture() {
  const contract = load('contract'), probes = load('probes');
  const current = { kind: 'environment', identity: 'reset-fixture-v1:isolated-memory' };
  const record = { id: 'qa-replay', criterionIds: ['single-use'], scope: ['reset'],
    contract: engineeringContractDigest(contract), binding: current, checkId: 'replay', kind: 'manual',
    artifact: 'tests/qa-projection.test.mjs: executable replay assertions', observedAt: '2026-10-22T00:00:00Z',
    reporter: 'fixture assessor', basis: 'A used reset link cannot be reused', support: 'verified', execution: 'failed' };
  return { contract, probes, current, evidence: { formatVersion: 1, records: [] }, record };
}
function project(input) {
  const response = evaluate({ formatVersion: 1, operation: 'qa-projection', input });
  assert.equal(response.ok, true, JSON.stringify(response));
  return response.result;
}

test('source plus application access retains a reproduced failure, build identity and implementation disagreement', () => {
  const { record, ...input } = fixture();
  const app = resetApplication();
  const first = app.redeem('synthetic-token'), replay = app.redeem('synthetic-token');
  assert.deepEqual(first, { accepted: true, changes: 1 });
  assert.deepEqual(replay, { accepted: true, changes: 2 });
  input.evidence.records = [{ ...record, id: 'implementation-check', kind: 'check',
    reporter: 'implementation fixture', execution: 'passed', artifact: 'First redemption only; replay not asserted' }, record];
  input.probes[0].evidenceIds = [record.id];
  const before = JSON.stringify(input);
  const view = project(input);
  assert.equal(view.criteria[0].id, 'single-use');
  assert.equal(view.sources[0].id, view.criteria[0].sourceIds[0]);
  assert.equal(view.criteria[0].conflict, true);
  assert.equal(view.criteria[0].acceptance, 'failed');
  assert.equal(view.coverage.complete, false);
  assert.equal(view.probes[0].records[0].execution, 'failed');
  assert.deepEqual(view.unexploredCriterionIds, []);
  assert.deepEqual(view, JSON.parse(JSON.stringify(view)));
  assert.equal(JSON.stringify(input), before);
});

test('requirements-only QA returns useful unexecuted probes without fabricated build or execution', () => {
  const { record, ...input } = fixture();
  input.current.identity = null;
  const planned = project(input);
  assert.equal(planned.probes[0].execution, 'not-run');
  assert.deepEqual(planned.unexploredCriterionIds, ['single-use']);
  assert.equal(planned.coverage.complete, false);
  input.probes[0].evidenceIds = [record.id];
  input.evidence.records = [{ ...record, binding: input.current, execution: 'not-available', support: 'unknown', artifact: 'Unavailable: browser, build identity and authorized synthetic account required' }];
  const unavailable = project(input);
  assert.equal(unavailable.probes[0].records[0].freshness, 'unknown');
  assert.equal(unavailable.probes[0].records[0].execution, 'not-available');
  assert.equal(unavailable.criteria[0].acceptance, 'gap');
  assert.equal(unavailable.criteria[0].conflict, false);
  assert.deepEqual(unavailable.unexploredCriterionIds, ['single-use']);
});

test('build, environment and contract changes cannot refresh an earlier QA observation', () => {
  const { record, ...input } = fixture();
  input.evidence.records = [{ ...record, execution: 'passed' }];
  input.probes[0].evidenceIds = [record.id];
  assert.equal(project(input).coverage.complete, true);
  for (const current of [{ kind: 'environment', identity: 'reset-fixture-v2:isolated-memory' },
    { kind: 'environment', identity: null }, { kind: 'repository', identity: record.binding.identity }]) {
    const view = project({ ...input, current });
    assert.equal(view.coverage.complete, false);
    assert.deepEqual(view.unexploredCriterionIds, ['single-use']);
  }
  input.contract.context.sources[0].identity = 'changed-requirement';
  assert.equal(project(input).probes[0].records[0].freshness, 'stale');
});

test('QA rejects dangling and misattributed probes and bounds its transient detail', () => {
  const { record, ...input } = fixture();
  const invalid = [
    probe => ({ ...probe, criterionIds: ['invented'] }),
    probe => ({ ...probe, evidenceIds: ['missing'] }),
    probe => ({ ...probe, actions: [] }),
    probe => ({ ...probe, expectedBasis: 'x'.repeat(1025) }),
    probe => ({ ...probe, observation: 'invented pass' }),
    probe => ({ ...probe, id: 'different', evidenceIds: [record.id] })
  ];
  input.evidence.records = [record];
  for (const change of invalid) {
    const response = evaluate({ formatVersion: 1, operation: 'qa-projection', input: { ...input, probes: [change(input.probes[0])] } });
    assert.equal(response.ok, false);
    assert.equal(response.error.code, 'INVALID_INPUT');
  }
  assert.equal(evaluate({ formatVersion: 1, operation: 'qa-projection', input: { ...input, probes: Array(33).fill(input.probes[0]) } }).ok, false);
});
