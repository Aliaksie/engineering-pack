import test from 'node:test';
import assert from 'node:assert/strict';
import { evaluateReviewPublication } from '../application/review.mjs';
const evaluate = event => evaluateReviewPublication(event);

test('read-only guard distinguishes direct commands from inert arguments', () => {
  for (const command of ['echo "git push"', "printf '%s' 'gh pr comment 1'", 'git log --grep="git push"']) {
    assert.equal(evaluate({ tool: 'shell', command, project: { review: { readOnly: true } } }).decision, 'allow', command);
  }
  for (const command of ['git -C "a project" push origin feature', 'git --no-pager push', 'git -c advice.pushUpdateRejected=false push', 'git --git-dir=repo push', 'git status && git push', 'echo "safe; git push"; gh pr comment 1 --body hi', '"git" push']) {
    assert.equal(evaluate({ tool: 'shell', command, project: { review: { readOnly: true } } }).decision, 'deny', command);
  }
});

test('read-only guard treats creation and implicit API writes as publication', () => {
  for (const command of ['gh pr create --fill', 'gh issue create -t x', 'glab mr create', 'gh api repos/o/r/issues/1/comments -f body=hi',
    'gh api repos/o/r/issues -F title=x', 'gh api repos/o/r/labels --raw-field=name=x', 'glab api projects/1/notes --field body=x',
    'gh api repos/o/r/pulls --input body.json', 'gh api repos/o/r -fname=x', 'gh api -XPOST repos/o/r/forks', 'gh api -X=put repos/o/r/topics']) {
    assert.equal(evaluate({ tool: 'shell', command, project: { review: { readOnly: true } } }).decision, 'deny', command);
  }
  for (const command of ['gh pr view 1', 'gh api repos/o/r/pulls', 'gh api -X GET search/issues -f q=x', 'gh api repos/o/r/pulls --method=GET -F per_page=5',
    'gh api graphql -f "query=query { viewer { login } }"', 'gh api repos/o/r/pulls --jq .[].title', 'gh api repos/o/r --paginate']) {
    assert.equal(evaluate({ tool: 'shell', command, project: { review: { readOnly: true } } }).decision, 'allow', command);
  }
});
