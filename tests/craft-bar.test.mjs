import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { BEGIN, END, apply, check, render, skillDirs } from '../tooling/sync-craft-bar.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
function fixture(t, contents = {}) {
  const directory = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-block-')));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  for (const file of ['tooling/sync-craft-bar.mjs', 'packages/craft-engineering/standards/craft-bar.md']) {
    const destination = path.join(directory, file);
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    fs.copyFileSync(path.join(root, file), destination);
  }
  const files = {};
  for (const [skill, body] of Object.entries(contents)) {
    files[skill] = path.join(directory, 'packages/craft-engineering/skills', skill, 'SKILL.md');
    fs.mkdirSync(path.dirname(files[skill]), { recursive: true });
    fs.writeFileSync(files[skill], body);
  }
  const invoke = (...args) => spawnSync(process.execPath, [path.join(directory, 'tooling/sync-craft-bar.mjs'), ...args], { encoding: 'utf8' });
  return { files, invoke };
}

test('configured engineering skills contain synchronized canonical blocks', () => {
  assert.ok(skillDirs().length > 0);
  assert.deepEqual(check(), []);
  for (const skill of skillDirs()) {
    const content = fs.readFileSync(path.join(root, 'packages/craft-engineering/skills', skill, 'SKILL.md'), 'utf8');
    assert.equal(apply(content, skill), content);
  }
});

test('render substitutes skill references and omits the standard maintainer header', () => {
  const standard = fs.readFileSync(path.join(root, 'packages/craft-engineering/standards/craft-bar.md'), 'utf8');
  const header = /^<!--[^]*?-->/.exec(standard)?.[0];
  const output = render('example-skill');
  assert.ok(output.startsWith(`${BEGIN}\n`));
  assert.ok(output.endsWith(`${END}\n`));
  assert.ok(output.includes('example-skill/<reference>'));
  assert.ok(!output.includes('{{SKILL}}'));
  if (header) assert.ok(!output.includes(header));
});

test('replacement preserves surrounding bytes and newline boundaries', () => {
  for (const newline of ['\n', '\r\n']) {
    for (const suffix of ['', newline, `${newline}${newline}## References${newline}Keep this.`]) {
      const before = `# Skill${newline}Human guidance.${newline}${newline}`;
      const input = `${before}${BEGIN}${newline}Outdated.${newline}${END}${suffix}`;
      const output = apply(input, 'foundations');
      const expected = render('foundations').slice(0, -1).replaceAll('\n', newline);
      assert.equal(output, before + expected + suffix);
      assert.equal(apply(output, 'foundations'), output);
    }
  }
  assert.equal(apply('No generated block.\n', 'foundations'), null);
});

test('malformed marker layouts are rejected instead of replacing unrelated text', () => {
  for (const body of [
    `${BEGIN}\nMissing end.`,
    `${END}\nMissing begin.`,
    `${END}\nUnrelated prose.\n${BEGIN}`,
    `${BEGIN}\n${BEGIN}\n${END}`,
    `${BEGIN}\n${END}\n${END}`,
    '<!-- craft-bar:begin\nMissing marker terminator.\n',
    `${BEGIN} inline text\n${END}`,
    `${BEGIN}\ninline ${END}`,
    `Prefix ${BEGIN}\n${END}`,
    `${BEGIN}\n${END} suffix`
  ]) assert.throws(() => apply(body, 'foundations'), /craft-bar/);
});

test('CLI checks drift without writing and synchronizes a valid block', (t) => {
  const body = `# Skill\n\n${BEGIN}\nReplace me.\n${END}\n\nKeep this.\n`;
  const { files, invoke } = fixture(t, { foundations: body });
  const checked = invoke('--check');
  assert.equal(checked.status, 1);
  assert.match(checked.stderr, /foundations\/SKILL\.md is out of date/);
  assert.equal(fs.readFileSync(files.foundations, 'utf8'), body);
  assert.equal(invoke().status, 0);
  assert.equal(fs.readFileSync(files.foundations, 'utf8'), apply(body, 'foundations'));
  assert.equal(invoke('--check').status, 0);
});

test('CLI refuses all writes when any selected skill has invalid markers', (t) => {
  const valid = `${BEGIN}\nOutdated.\n${END}\n`;
  const invalid = `${END}\nDo not remove this.\n${BEGIN}\n`;
  const { files, invoke } = fixture(t, { foundations: valid, 'backend-engineering': invalid });
  for (const args of [['--check'], []]) {
    const result = invoke(...args);
    assert.equal(result.status, 1);
    assert.match(result.stderr, /end marker precedes/);
    assert.equal(fs.readFileSync(files.foundations, 'utf8'), valid);
    assert.equal(fs.readFileSync(files['backend-engineering'], 'utf8'), invalid);
  }
});

test('CLI reports an empty target set and missing markers', (t) => {
  const empty = fixture(t);
  assert.match(empty.invoke('--check').stderr, /check is reading nothing/);
  assert.equal(empty.invoke().status, 1);
  const missing = fixture(t, { foundations: '# No block\n' });
  assert.equal(missing.invoke('--check').status, 1);
  assert.match(missing.invoke().stderr, /has no craft-bar markers/);
});
