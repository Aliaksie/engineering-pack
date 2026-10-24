import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { locate } from '../application/discovery/locate.mjs';

function fixture(t, git = true) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-locate-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  if (git) assert.equal(spawnSync('git', ['init', '-q'], { cwd: root }).status, 0);
  return root;
}
function write(root, name, text) {
  fs.mkdirSync(path.dirname(path.join(root, name)), { recursive: true });
  fs.writeFileSync(path.join(root, name), text);
}

test('locate batches literal terms with compact evidence and respects Git ignores and scopes', t => {
  const root = fixture(t);
  write(root, '.gitignore', 'secret/\n');
  write(root, 'secret/private.txt', 'alpha beta');
  write(root, 'source/space ü.txt', 'alpha\r\nalpha beta\r\n');
  write(root, 'other.txt', 'beta');
  const result = locate(root, { terms: ['alpha', 'beta', 'alpha'], paths: ['source'] });
  assert.equal(result.inventory, 'git');
  assert.equal(result.filesScanned, 1);
  assert.deepEqual(result.matches.map(({ line, termIndexes }) => ({ line, terms: termIndexes.map(index => result.terms[index]) })), [{ line: 1, terms: ['alpha'] }, { line: 2, terms: ['alpha', 'beta'] }]);
  assert.equal(result.matches[0].path, 'source/space ü.txt');
  assert.ok(locate(root, { terms: ['alpha'] }).matches.every(match => !match.path.startsWith('secret/')));
});

test('locate bounds work and output, excludes binary and oversized content', t => {
  const root = fixture(t);
  write(root, 'a.txt', 'needle'.repeat(100) + '\nneedle');
  write(root, 'binary', Buffer.from('needle\0'));
  write(root, 'huge', 'needle'.repeat(50_000));
  const result = locate(root, { terms: ['needle'] });
  assert.equal(result.skipped.binary, 1);
  assert.equal(result.skipped.oversized, 1);
  assert.equal(result.matches[0].text.length, 240);
  assert.equal(result.matches[0].snippetTruncated, true);
  assert.ok(locate(root, { terms: ['needle'], maxMatches: 1 }).truncation.includes('maxMatches'));
  assert.ok(locate(root, { terms: ['needle'], maxFiles: 1 }).truncation.includes('maxFiles'));
  assert.ok(locate(root, { terms: ['needle'], maxBytes: 2 }).truncation.includes('maxBytes'));
});

test('non-Git search requires bounded explicit scope and does not follow symlinks', t => {
  const root = fixture(t, false);
  const outside = fixture(t, false);
  write(outside, 'outside.txt', 'needle outside');
  write(root, 'src/inside.txt', 'needle inside');
  write(root, 'src/node_modules/ignored.txt', 'needle');
  fs.symlinkSync(outside, path.join(root, 'src', 'linked'), 'junction');
  assert.throws(() => locate(root, { terms: ['needle'] }), /explicit paths/);
  assert.throws(() => locate(root, { terms: ['needle'], paths: ['src/linked'] }), /Symlink scope/);
  const result = locate(root, { terms: ['needle'], paths: ['src'] });
  assert.equal(result.inventory, 'explicit-paths');
  assert.deepEqual(result.matches.map(match => match.path), ['src/inside.txt']);
  assert.equal(result.skipped.symlink, 1);
  assert.equal(result.skipped.excludedDirectory, 1);
  for (const bad of ['../outside', '/tmp', 'C:\\outside']) assert.throws(() => locate(root, { terms: ['needle'], paths: [bad] }), /relative scopes/);
});

test('Git inventory does not read linked outside content and rejects invalid budgets', t => {
  const root = fixture(t);
  const outside = fixture(t, false);
  write(outside, 'private.txt', 'needle outside');
  write(root, 'inside.txt', 'needle inside');
  fs.symlinkSync(outside, path.join(root, 'linked'), 'junction');
  const result = locate(root, { terms: ['needle'] });
  assert.deepEqual(result.matches.map(match => match.path), ['inside.txt']);
  for (const options of [{ terms: [] }, { terms: ['needle'], maxBytes: 0 }, { terms: ['needle'], maxFiles: Infinity }, { terms: ['needle'], maxMatches: 1.5 }]) {
    assert.throws(() => locate(root, options));
  }
});


test('equivalent canonical scopes select the same evidence in Git and explicit inventories', t => {
  for (const useGit of [true, false]) {
    const root = fixture(t, useGit);
    write(root, 'src/main.mjs', 'export const marker = "needle";\n');
    write(root, 'elsewhere/main.mjs', 'needle outside scope');
    const baseline = locate(root, { terms: ['needle'], paths: ['src'] });
    for (const spelling of ['./src', 'src/.', '././src', './src/./', 'src//']) {
      const result = locate(root, { terms: ['needle'], paths: [spelling] });
      assert.deepEqual(result.paths, ['src']);
      assert.deepEqual(result.matches, baseline.matches, `${useGit ? 'Git' : 'explicit'} scope ${spelling}`);
    }
    const outside = fixture(t, false);
    write(outside, 'private.mjs', 'needle');
    fs.symlinkSync(outside, path.join(root, 'linked'), 'junction');
    assert.throws(() => locate(root, { terms: ['needle'], paths: ['./linked/.'] }), /Symlink scope/);
    assert.throws(() => locate(root, { terms: ['needle'], paths: ['src/../elsewhere'] }), /relative scopes/);
  }
});

test('locate preserves filesystem error codes for scopes below a regular file', t => {
  const root = fixture(t);
  write(root, 'source', 'needle');
  assert.throws(() => locate(root, { terms: ['needle'], paths: ['source/child'] }), { code: 'ENOTDIR' });
});
