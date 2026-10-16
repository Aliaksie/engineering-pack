import test from 'node:test';
import assert from 'node:assert/strict';
import { derive } from '../tools/craft-review-inbox/bin/derive.mjs';

const date = (day) => `2026-08-${String(day).padStart(2, '0')}T09:00:00Z`;
const connection = (...items) => ({ nodes: items });
const remark = (login, day, body = 'Please explain the retry limit.') => ({ author: { login }, createdAt: date(day), body });
function observation({ remarks = [], approvals = [], threads = [], committed = 1, sha = 'revision-a' } = {}) {
  return { number: 73, title: 'Limit retry duration', author: { login: 'developer' },
    headRefOid: sha, commits: connection({ commit: { oid: sha, committedDate: date(committed) } }),
    comments: connection(...remarks), reviews: connection(...approvals), reviewThreads: connection(...threads) };
}
function calculate(pr, extras = {}) {
  return derive(pr, { me: 'auditor', repoShort: 'service', repoName: 'payments', now: date(20), staleDays: 7, ...extras });
}
const mine = remark('auditor', 17);
const approval = (day, body = '') => ({ author: { login: 'maintainer' }, submittedAt: date(day), state: 'APPROVED', body });

const cases = [
  ['NEW', observation(), {}],
  ['DRAFT_READY', observation(), { artifacts: { comment_mtime: date(18) } }],
  ['RE_REVIEW_NEEDED', observation({ sha: 'revision-b' }), { prev: { reviewed_head_sha: 'revision-a' }, artifacts: { comment_mtime: date(18) } }],
  ['FIXED_PENDING_VERIFY', observation({ remarks: [mine], committed: 18 }), {}],
  ['ANSWERED', observation({ remarks: [mine, remark('developer', 18)] }), {}],
  ['CLEAN', observation({ remarks: [mine], approvals: [approval(18)] }), {}],
  ['STALE_MINE', observation({ remarks: [remark('auditor', 2)] }), {}],
  ['AWAITING_AUTHOR', observation({ remarks: [mine] }), {}]
];
for (const [expected, pr, local] of cases) test(`inbox recommendation ${expected}`, () => assert.equal(calculate(pr, local).status, expected));

test('approvals and substantive responses are separate observations', () => {
  assert.equal(calculate(observation({ remarks: [mine], approvals: [approval(16)] })).status, 'AWAITING_AUTHOR');
  const silent = calculate(observation({ remarks: [mine], approvals: [approval(18, '  ')] }));
  assert.equal(silent.status, 'CLEAN'); assert.equal(silent.author_reply_after, false);
  assert.equal(calculate(observation({ remarks: [mine], approvals: [approval(18, 'The limit is now documented.')] })).status, 'ANSWERED');
});

test('thread participation and resolution preserve a concern until verification', () => {
  const thread = { id: 'thread-8', path: 'retry.mjs', line: 14, isResolved: false, isOutdated: false, comments: connection(mine) };
  const initial = calculate(observation({ remarks: [mine], approvals: [approval(18)], threads: [thread] }));
  assert.equal(initial.status, 'AWAITING_AUTHOR'); assert.equal(initial.open_threads_mine, 1);
  const resolved = calculate(observation({ remarks: [mine], threads: [{ ...thread, isResolved: true }] }), { prev: initial });
  assert.equal(resolved.status, 'FIXED_PENDING_VERIFY'); assert.deepEqual(resolved.newly_resolved_threads, ['thread-8']);
  assert.equal(calculate(observation({ remarks: [mine], threads: [{ ...thread, isResolved: true }] }), { prev: resolved }).newly_resolved_threads.length, 0);
});

test('draft posting requires explicit local evidence rather than timestamp inference', () => {
  const pr = observation({ remarks: [remark('auditor', 19)] });
  assert.equal(calculate(pr, { artifacts: { comment_mtime: date(18) } }).status, 'DRAFT_READY');
  const posted = calculate(pr, { artifacts: { comment_mtime: date(18) }, prev: { posted_at: date(19) } });
  assert.equal(posted.status, 'AWAITING_AUTHOR'); assert.equal(posted.posted_inferred, false);
  assert.equal(calculate(pr, { prev: { posted_inferred: true, posted_at: date(19) } }).posted_inferred, true);
});

test('rescan preserves local identity and appends history only on a recommendation change', () => {
  const pr = observation({ remarks: [mine] });
  const previous = { ...calculate(pr), reviewed_head_sha: 'revision-a', reviewed_at: date(18), local_verdict: 'COMMENT', first_seen: date(3) };
  const repeat = calculate(pr, { prev: previous });
  assert.deepEqual(repeat.history, previous.history);
  assert.equal(repeat.reviewed_head_sha, 'revision-a'); assert.equal(repeat.local_verdict, 'COMMENT'); assert.equal(repeat.first_seen, date(3));
  const next = calculate(observation({ remarks: [mine], committed: 19 }), { prev: repeat });
  assert.equal(next.history.length, repeat.history.length + 1);
});

test('missing author and malformed dates cannot create positive approval or reply evidence', () => {
  const state = calculate(observation({ remarks: [{ author: { login: 'auditor' }, createdAt: 'invalid' }], approvals: [{ state: 'APPROVED', submittedAt: date(19) }] }));
  assert.equal(state.status, 'NEW'); assert.equal(state.approved_by_other, false); assert.equal(state.my_last_signal_at, null);
  assert.equal(calculate(null).status, 'NEW');
});
