import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { run } from './craft.mjs';
import { DOC, SCHEMA, render } from '../tooling/project-config-doc.mjs';

// The project schema is the configuration reference: every field described, its defaults the
// ones `craft settings` applies, and docs/PROJECT-CONFIG.md generated from it.
const root = fileURLToPath(new URL('../', import.meta.url));
const schema = JSON.parse(fs.readFileSync(path.join(root, SCHEMA), 'utf8'));
const SWITCHES = ['routing', 'checkpoint', 'workspace', 'efficiency'];

function fields(node, at = [], out = []) {
  const resolved = node.$ref ? schema.$defs[node.$ref.replace('#/$defs/', '')] : node;
  for (const option of resolved.oneOf ?? []) fields(option, at, out);
  for (const [name, child] of Object.entries(resolved.properties ?? {})) {
    out.push({ at: [...at, name], node: child });
    fields(child, [...at, name], out);
  }
  if (resolved.items && typeof resolved.items === 'object') fields(resolved.items, [...at, '[]'], out);
  if (resolved.additionalProperties && typeof resolved.additionalProperties === 'object') fields(resolved.additionalProperties, [...at, '{}'], out);
  return out;
}

function settingsFor(t, project) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-schema-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  fs.mkdirSync(path.join(dir, '.craft'));
  fs.writeFileSync(path.join(dir, '.craft/project.json'), JSON.stringify(project));
  return run(['settings', '--root', dir, '--preferences', path.join(dir, 'none.json')]);
}

test('docs/PROJECT-CONFIG.md is generated from the current schema', () => {
  const current = fs.readFileSync(path.join(root, DOC), 'utf8').replace(/\r\n/g, '\n');
  assert.equal(current, render(schema), 'run npm run docs:config');
});

test('every field the project schema accepts is described', () => {
  const missing = fields(schema).filter(({ node }) => !node.description).map(({ at }) => at.join('.'));
  assert.deepEqual(missing, []);
});

test('schema defaults are the defaults craft applies', t => {
  const { settings } = settingsFor(t, { formatVersion: 1 });
  const declared = new Map(fields(schema)
    .filter(({ at, node }) => SWITCHES.includes(at[0]) && node.default !== undefined)
    .map(({ at, node }) => [at.join('.'), node.default]));
  const applied = new Map();
  (function leaves(value, at) {
    if (value && typeof value === 'object' && !Array.isArray(value)) for (const [k, v] of Object.entries(value)) leaves(v, [...at, k]);
    else applied.set(at.join('.'), value);
  })(Object.fromEntries(SWITCHES.map(name => [name, settings[name]])), []);
  assert.deepEqual(Object.fromEntries(declared), Object.fromEntries(applied));
});

test('a project file may link the schema; craft ignores the link', t => {
  const example = JSON.parse(fs.readFileSync(path.join(root, 'examples/project/.craft/project.json'), 'utf8'));
  assert.equal(example.$schema, path.relative(path.join(root, 'examples/project/.craft'), path.join(root, SCHEMA)).replaceAll(path.sep, '/'));
  const { settings, origins } = settingsFor(t, example);
  assert.equal('$schema' in settings, false);
  assert.equal('$schema' in origins, false);
  assert.throws(() => settingsFor(t, { formatVersion: 1, $schema: 1 }), /\$schema must be a string/);
});
