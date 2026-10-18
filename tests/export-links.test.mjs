import assert from 'node:assert/strict';
import path from 'node:path';
import test from 'node:test';
import { rewriteLinks } from '../tooling/export/export.mjs';

// Use both path implementations so Windows regressions fail on every CI runner.
for (const [name, paths, root] of [
  ['Windows', path.win32, 'C:\\temporary'],
  ['POSIX', path.posix, '/temporary']
]) {
  test(`${name} build links remain relative and independent of the staging directory`, () => {
    const render = stageName => {
      const stage = paths.join(root, stageName);
      const source = paths.join(stage, 'packages/craft-delivery/skills/write-adr/SKILL.md');
      const target = paths.join(stage, '.cursor/skills/craft-delivery-write-adr/SKILL.md');
      return rewriteLinks('[package instructions](../../AGENTS.md#policy)', source, target, resolved => resolved, paths);
    };
    assert.equal(render('build-one'), '[package instructions](../../../packages/craft-delivery/AGENTS.md#policy)');
    assert.equal(render('build-two'), render('build-one'));
  });

  test(`${name} link rewriting preserves external URLs and mapped documentation URLs`, () => {
    const file = paths.join(root, 'SKILL.md');
    const external = '[docs](https://example.org/docs#usage) [section](#local) [contact](mailto:team@example.org)';
    assert.equal(rewriteLinks(external, file, file, () => assert.fail('external links must not be rebased'), paths), external);
    assert.equal(rewriteLinks('[guide](guide.md#usage)', file, file,
      () => 'https://example.org/guide.md', paths), '[guide](https://example.org/guide.md#usage)');
  });
}
