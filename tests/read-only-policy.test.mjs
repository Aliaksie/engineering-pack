import test from 'node:test';
import assert from 'node:assert/strict';
import { evaluate } from '../packages/craft-review/hooks/read-only.mjs';

test('read-only guard distinguishes direct commands from inert arguments', () => {
  for (const command of ['echo "git push"', "printf '%s' 'gh pr comment 1'", 'git log --grep="git push"']) {
    assert.equal(evaluate({ tool: 'shell', command, project: { review: { readOnly: true } } }).decision, 'allow', command);
  }
  for (const command of ['git -C "a project" push origin feature', 'git --no-pager push', 'git -c advice.pushUpdateRejected=false push', 'git --git-dir=repo push', 'git status && git push', 'echo "safe; git push"; gh pr comment 1 --body hi', '"git" push']) {
    assert.equal(evaluate({ tool: 'shell', command, project: { review: { readOnly: true } } }).decision, 'deny', command);
  }
});
