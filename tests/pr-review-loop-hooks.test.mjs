import test from 'node:test';
import assert from 'node:assert/strict';
import { evaluate as guard } from '../tools/craft-review-inbox/hooks/gh-guard.mjs';
import { evaluate as trim } from '../tools/craft-review-inbox/hooks/gh-trim.mjs';

const decision = (policy, command) => policy({ command }).decision;
test('administration restrictions do not accept a write exception', () => {
  for (const command of ['gh repo delete sample/service', 'gh secret set KEY', 'gh api --method DELETE repos/sample/service', 'PR_LOOP_ALLOW_WRITE=1 gh repo archive sample/service']) {
    assert.equal(decision(guard, command), 'deny', command);
  }
});
test('direct posting needs a deliberate marker on the same invocation', () => {
  assert.equal(decision(guard, 'gh pr review 73 --approve'), 'deny');
  assert.equal(decision(guard, 'PR_LOOP_ALLOW_WRITE=1 gh pr review 73 --approve'), 'allow');
  assert.equal(decision(guard, 'echo PR_LOOP_ALLOW_WRITE=1; gh pr review 73 --approve'), 'deny');
  assert.equal(decision(guard, 'gh pr comment 73 --body "PR_LOOP_ALLOW_WRITE=1"'), 'deny');
});
test('literal mentions do not become executable GitHub commands', () => {
  for (const command of ['echo "gh repo delete sample/service"', 'git commit -m "gh pr merge 73"', 'cat gh-notes.txt']) assert.equal(decision(guard, command), 'allow');
  assert.equal(decision(guard, 'echo hello && gh pr merge 73'), 'deny');
  assert.equal(decision(guard, 'gh --hostname example.test pr review 73 --approve'), 'deny');
});
test('GraphQL read recognition requires a visible query document', () => {
  assert.equal(decision(guard, 'gh api graphql -f "query=query Agenda { viewer { login } }"'), 'allow');
  assert.equal(decision(guard, 'gh api graphql -f "query=mutation Update { changeThing { id } }"'), 'deny');
  assert.equal(decision(guard, 'gh api graphql -f query=@private.graphql'), 'deny');
  assert.equal(decision(guard, 'gh api graphql -f query="$DOCUMENT"'), 'deny');
  assert.equal(decision(guard, 'gh api repos/sample/service -f title=changed'), 'deny');
  assert.equal(decision(guard, 'gh api repos/sample/service --method GET -f page=2'), 'allow');
});
test('scratch push advice leaves the consumer project checkout alone', () => {
  assert.equal(decision(guard, 'git -C .pr-loop/scratch/work/service push origin HEAD'), 'deny');
  assert.equal(decision(guard, 'git -C consumer/service push origin HEAD'), 'allow');
});
test('output advice distinguishes broad reads from projected forms', () => {
  const pairs = [
    ['gh pr diff 73', 'gh pr diff 73 --name-only'],
    ['gh pr view 73', 'gh pr view 73 --json number,title'],
    ['gh run view 93 --log', 'gh run view 93 --log-failed'],
    ['gh api repos/sample/service/pulls/73/files', 'gh api repos/sample/service/pulls/73/files --jq ".[].filename"']
  ];
  for (const [broad, projected] of pairs) {
    assert.equal(decision(trim, broad), 'deny'); assert.equal(decision(trim, projected), 'allow');
    assert.match(trim({ command: broad }).reason, /PR_LOOP_RAW=1/);
  }
});
test('raw-output exception is invocation-scoped and quoted text stays inert', () => {
  assert.equal(decision(trim, 'PR_LOOP_RAW=1 gh pr diff 73'), 'allow');
  assert.equal(decision(trim, 'echo PR_LOOP_RAW=1; gh pr diff 73'), 'deny');
  assert.equal(decision(trim, 'echo "gh pr diff 73"'), 'allow');
  assert.equal(decision(trim, 'gh pr diff 73 > patch.txt'), 'allow');
  assert.equal(decision(trim, 'gh pr diff 73 | head -40'), 'allow');
});
test('normalized argv is supported without parsing or running shell text', () => {
  assert.equal(guard({ argv: ['gh', 'pr', 'comment', '73', '--body', 'hello'] }).decision, 'deny');
  assert.equal(trim({ argv: ['gh', 'pr', 'view', '73', '--json=title'] }).decision, 'allow');
  assert.equal(guard({ command: 7 }).decision, 'allow');
});
