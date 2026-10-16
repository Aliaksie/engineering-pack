import test from 'node:test';
import assert from 'node:assert/strict';
import { validateReview, publicationPlan } from '../packages/craft-review/lib/review.mjs';

const input = {
  formatVersion: 1, provider: 'github', repository: 'sample/project', changeId: 12,
  baseSha: 'a'.repeat(40), headSha: 'b'.repeat(40), mergeBase: 'a'.repeat(40),
  files: ['worker.mjs'], diff: 'Synthetic contract fixture; no semantic review evidence.'
};
const finding = (severity, id = severity) => ({
  id, severity, path: 'worker.mjs', side: 'new', line: 1,
  summary: 'Synthetic finding used to validate the artifact contract.', evidence: 'Fixture only.'
});
const review = (overrides = {}) => ({
  baseSha: input.baseSha, headSha: input.headSha,
  summary: 'Behavior has not been exercised; additional verification is needed.',
  recommendation: 'needs-verification', findings: [], verification: ['No project checks or model review ran.'],
  ...overrides
});

test('review artifacts require one canonical recommendation and canonical severity names', () => {
  for (const recommendation of [undefined, null, '', 'APPROVE', 'comment', 'request_changes']) {
    assert.throws(() => validateReview(input, review({ recommendation })), /recommendation/);
  }
  for (const severity of ['error', 'warning', 'info', 'critical', '', null, ['high']]) {
    assert.throws(() => validateReview(input, review({ findings: [finding(severity, 'invalid-severity-fixture')] })), /severity/);
  }
  assert.throws(() => validateReview(input, review({ findings: [null] })), /finding IDs/);
});

test('validation returns severity counts and the recommendation without judging the evidence', () => {
  const result = validateReview(input, review({
    recommendation: 'request-changes',
    findings: [finding('high'), finding('medium'), finding('low', 'L1'), finding('low', 'L2')]
  }));
  assert.equal(result.recommendation, 'request-changes');
  assert.deepEqual(result.severityCounts, { high: 1, medium: 1, low: 2 });
  assert.equal(result.findings, 4);
  assert.match(result.limitation, /Structural validation only/);
});

test('approval rejects high or medium findings but allows low findings and clean reviews', () => {
  for (const severity of ['high', 'medium']) {
    assert.throws(() => validateReview(input, review({ recommendation: 'approve', findings: [finding(severity)] })), /conflicts/);
  }
  for (const findings of [[], [finding('low')]]) {
    const result = validateReview(input, review({ recommendation: 'approve', findings }));
    assert.equal(result.valid, true);
    assert.equal(result.recommendation, 'approve');
  }
});

test('needs-verification is valid without inventing a finding or a completed check', () => {
  const artifact = review();
  assert.equal(validateReview(input, artifact).recommendation, 'needs-verification');
  assert.deepEqual(artifact.verification, ['No project checks or model review ran.']);
  assert.deepEqual(validateReview(input, artifact).severityCounts, { high: 0, medium: 0, low: 0 });
});

test('optional context accepts an empty list or substantive notes and rejects malformed values', () => {
  for (const context of [undefined, [], ['A configured runtime could not be checked.']]) {
    assert.equal(validateReview(input, review({ context })).valid, true);
  }
  for (const context of [null, 'note', {}, [''], ['  '], [null], [4]]) {
    assert.throws(() => validateReview(input, review({ context })), /context/);
  }
});

test('publication plans preserve the human recommendation and context while remaining comment-only', () => {
  const artifact = review({ recommendation: 'approve', context: ['This is a structural fixture, not an executed review.'] });
  const target = { currentHead: input.headSha, currentBase: input.baseSha, authorized: true };
  const plan = publicationPlan(input, artifact, target);
  assert.equal(plan.recommendation, 'approve');
  assert.deepEqual(plan.context, artifact.context);
  assert.equal(plan.summary, artifact.summary);
  assert.deepEqual(plan.verification, artifact.verification);
  assert.equal(plan.action, 'comment-only');
  assert.equal(plan.execution, 'not-published');
  assert.equal(Object.hasOwn(publicationPlan(input, review(), target), 'context'), false);
  assert.deepEqual(publicationPlan(input, review({ context: [] }), target).context, []);
  assert.throws(() => publicationPlan(input, artifact, { ...target, authorized: false }), /authorization/);
  assert.throws(() => publicationPlan(input, artifact, { ...target, currentHead: input.baseSha }), /changed/);
  assert.throws(() => publicationPlan(input, artifact, { ...target, currentBase: input.headSha }), /changed/);
});
