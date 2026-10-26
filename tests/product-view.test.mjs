import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { acceptanceView, contractDigest } from './acceptance-view.mjs';

const fixture = () => JSON.parse(fs.readFileSync(new URL('../examples/evals/product-views/input.json', import.meta.url), 'utf8'));
function project(view, input) {
  const response = acceptanceView(view, input);
  assert.equal(response.ok, true, JSON.stringify(response));
  return response.result;
}

test('product and architecture readers share IDs, conflicts, uncertainty and source provenance', () => {
  const input = fixture(), before = JSON.stringify(input);
  const product = project('product', input), architecture = project('architecture', input);
  for (const field of ['contract', 'criteria', 'sources', 'constraints', 'decisions', 'questions', 'coverage', 'dependencies']) {
    assert.deepEqual(product[field], architecture[field]);
  }
  assert.equal(product.discoveryCoverage, 'incomplete');
  assert.equal(product.deliveryGaps[0].id, 'single-use');
  assert.equal(product.criteria[0].acceptance.status, 'gap');
  assert.equal(product.unresolvedDecisions[0].id, 'redemption-owner');
  assert.equal(product.coverage.complete, false);
  assert.equal(product.dependencies[0].sources[0].provenance, 'declared');
  assert.equal(product.dependencies[1].sources[0].provenance, 'observed');
  assert.equal(architecture.adrSources[0].status, 'proposed');
  assert.equal(architecture.consequences[0].status, 'proposed');
  assert.deepEqual(architecture.boundaries[0].conflicts, ['local-state']);
  assert.equal(JSON.stringify(input), before);
  assert.deepEqual(architecture, JSON.parse(JSON.stringify(architecture)));
});

test('views preserve passed, failed, unavailable and stale evidence without resolving product choices', () => {
  const input = fixture();
  input.current.identity = 'build-v1';
  const record = { id: 'replay-check', criterionIds: ['single-use'], scope: ['reset'],
    contract: contractDigest(input.contract), binding: { ...input.current }, checkId: 'replay',
    kind: 'manual', artifact: 'Synthetic replay observation', observedAt: '2026-10-22T00:00:00Z',
    reporter: 'fixture author', basis: 'Replay a synthetic link', support: 'verified', execution: 'passed' };
  for (const view of ['product', 'architecture']) {
    for (const [execution, expected] of [['passed', 'verified'], ['failed', 'failed'], ['not-run', 'gap'], ['not-available', 'gap']]) {
      input.evidence.records = [{ ...record, execution }];
      const result = project(view, input);
      assert.equal(result.criteria[0].acceptance.status, expected);
      assert.equal(result.coverage.complete, false); // unresolved blocking decision
      assert.equal(result.coverage.records[0].execution, execution);
    }
    input.evidence.records = [record];
    assert.equal(project(view, { ...input, current: { ...input.current, identity: 'build-v2' } }).coverage.records[0].freshness, 'stale');
    assert.equal(project(view, { ...input, current: { ...input.current, identity: null } }).coverage.records[0].freshness, 'unknown');
    const changed = structuredClone(input);
    changed.contract.context.sources[0].identity = 'request-v2';
    assert.equal(project(view, changed).criteria[0].acceptance.status, 'gap');
  }
});

test('requirements-only views need neither runtime access nor invented architectural facts', () => {
  const input = fixture();
  input.contract.context.constraints = [];
  input.contract.context.sources = [input.contract.context.sources[0]];
  delete input.focus;
  const view = project('architecture', input);
  assert.deepEqual(view.dependencies, []);
  assert.deepEqual(view.adrSources, []);
  assert.deepEqual(view.boundaries, []);
  assert.equal(view.evidenceGaps[0].status, 'gap');
  assert.equal(view.discoveryCoverage, 'incomplete');
  assert.ok(view.questions.length);
});

test('focus rejects invented facts, dangling references, duplicate IDs and oversized selections', () => {
  for (const view of ['product', 'architecture']) {
    for (const focus of [null, [], { dependency: [] }, { dependencies: null },
      { dependencies: ['missing'] }, { dependencies: ['shared-store', 'shared-store'] },
      { dependencies: Array(33).fill('shared-store') }, { boundaries: [42] }]) {
      const response = acceptanceView(view, { ...fixture(), focus });
      assert.equal(response.ok, false);
      assert.match(response.error, /^product:invalid-(focus|reference)$/);
    }
    assert.equal(acceptanceView(view, { ...fixture(), contract: null }).error, 'product:missing-contract');
  }
});
