import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { validateReferences } from '../tooling/lib/cross-references.mjs';

const built = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../dist/claude');
function scenario(t, fields, body) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'component-references-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const write = (name, text) => {
    const file = path.join(root, name);
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, text);
  };
  write('plugins/library/skills/inspect/SKILL.md', '---\nname: inspect\ndescription: Inspect work.\n---\nMethod.\n');
  write('plugins/worker/agents/reviewer.md', `---\nname: reviewer\ndescription: Reviews work.\n${fields}\n---\n${body}\n`);
  const catalog = { plugins: [{ name: 'library', source: './plugins/library' }, { name: 'worker', source: './plugins/worker' }] };
  return { root, catalog, write, errors: () => validateReferences(root, catalog, []) };
}

test('all generated plugin references resolve', () => {
  const catalog = JSON.parse(fs.readFileSync(path.join(built, '.claude-plugin/marketplace.json'), 'utf8'));
  assert.deepEqual(validateReferences(built, catalog, []), []);
});

test('qualified references resolve skills, commands and agents within the cited plugin', (t) => {
  const context = scenario(t, 'tools: Skill', 'Use `library:inspect`, `library:dispatch` and `worker:reviewer`.');
  context.write('plugins/library/commands/dispatch.md', 'Dispatch.\n');
  assert.deepEqual(context.errors(), []);
  context.write('plugins/library/commands/dispatch.md', 'Use `library:missing`.\n');
  assert.ok(context.errors().some((error) => error.includes('does not ship')));
});

test('agent skill references need invocation permission or a valid preload', (t) => {
  for (const fields of ['tools: Read', 'tools: Read, Skill\ndisallowedTools: Skill']) {
    const context = scenario(t, fields, 'Use `inspect` and `library:inspect`.');
    assert.ok(context.errors().some((error) => error.includes('cannot load them')));
  }
  for (const fields of ['tools: Read, Skill', '', 'tools: Read\nskills:\n  - inspect', 'tools: Read\nskills: [library:inspect]']) {
    assert.deepEqual(scenario(t, fields, 'Use `library:inspect` and `inspect`.').errors(), []);
  }
});

test('invalid preloads are rejected even when Skill invocation is permitted', (t) => {
  for (const citation of ['missing', 'library:missing', 'library:inspect:extra']) {
    const context = scenario(t, `tools: Skill\nskills: ${citation}`, 'Choose a method.');
    assert.ok(context.errors().some((error) => error.includes('no plugin in this marketplace ships')));
  }
});

test('preloading one plugin skill does not make an identically named skill in another plugin reachable', (t) => {
  const context = scenario(t, 'tools: Read\nskills: worker:inspect', 'Use `library:inspect`.');
  context.write('plugins/worker/skills/inspect/SKILL.md', '---\nname: inspect\ndescription: A distinct method.\n---\n');
  assert.ok(context.errors().some((error) => error.includes('library:inspect') && error.includes('cannot load them')));
});

test('reference scanning rejects symlinked plugin directories and tolerates malformed catalog entries', (t) => {
  const context = scenario(t, 'tools: Skill', 'Use `library:inspect`.');
  fs.symlinkSync(path.join(context.root, 'plugins/library'), path.join(context.root, 'alias'), 'junction');
  const errors = validateReferences(context.root, { plugins: [null, false, { name: 'alias', source: './alias' }] }, []);
  assert.ok(errors.some((error) => error.includes('must be an object')));
  assert.ok(errors.some((error) => error.includes('symbolic link')));
  assert.deepEqual(validateReferences(context.root, null, []), []);
});
