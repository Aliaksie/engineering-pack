import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';

const workflow = `name: Linter probe
on: push
permissions:
  contents: read
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - run: echo ok
`;
function lint(input, shellcheck = false) {
  const result = spawnSync('actionlint', ['-no-color', ...(shellcheck ? [] : ['-shellcheck=']), '-pyflakes=', '-'], {
    input, encoding:'utf8', timeout:10000, maxBuffer:65536, shell:false
  });
  assert.ifError(result.error);
  assert.equal(result.signal,null);
  return { status:result.status, output:result.stdout+result.stderr };
}
test('workflow linter accepts a valid workflow',()=>{
  const result=lint(workflow);
  assert.equal(result.status,0,result.output);
});
for (const [name,input,category] of [
  ['malformed YAML',workflow+'broken: [\n',/\[syntax-check\]/],
  ['invalid expression context',workflow.replace('echo ok','echo ${{ nonexistent.value }}'),/\[expression\]/],
  ['missing job dependency',workflow.replace('    runs-on:', '    needs: missing-job\n    runs-on:'),/\[job-needs\]/]
]) test(`workflow linter rejects ${name}`,()=>{
  const result=lint(input);
  assert.equal(result.status,1,result.output);
  assert.match(result.output,category);
});
test('workflow linter detects unsafe shell expansion through ShellCheck',()=>{
  const result=lint(workflow.replace('echo ok','ls $GITHUB_WORKSPACE'),true);
  assert.equal(result.status,1,result.output);
  assert.match(result.output,/\[shellcheck\]/);
  assert.match(result.output,/SC2086/);
});
