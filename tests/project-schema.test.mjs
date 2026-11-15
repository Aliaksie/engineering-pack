import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { run } from './craft.mjs';
import { DOC, LAYER_FIELDS, LAYER_SCHEMA, SCHEMA, layerSchema, render } from '../tooling/project-config-doc.mjs';

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

test('the layer schema is the project schema narrowed to the fields a layer may set', t => {
  const current = JSON.parse(fs.readFileSync(path.join(root, LAYER_SCHEMA), 'utf8'));
  assert.deepEqual(current, layerSchema(schema), 'run npm run docs:config');
  const engine = fs.readFileSync(path.join(root, 'engine/src/layers.rs'), 'utf8');
  const declared = engine.match(/pub const FIELDS: \[&str; \d+\] = \[([^\]]*)\]/)[1].match(/"[^"]+"/g).map(name => JSON.parse(name));
  assert.deepEqual(LAYER_FIELDS, declared);
  // An organization file that links it and sets a standard of each layer form is accepted.
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-layer-schema-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const organization = path.join(dir, 'organization.json');
  fs.writeFileSync(organization, JSON.stringify({ $schema: `../${LAYER_SCHEMA}`, formatVersion: 1, standards: [
    { id: 'commits', anchor: 'conventional-commits' },
    { id: 'review', checklist: ['Tests cover the change'] },
    { id: 'licenses', run: { command: 'git', args: ['--version'] } }] }), { mode: 0o600 });
  const report = run(['config', '--root', dir, '--organization', organization, '--preferences', path.join(dir, 'none.json')]);
  assert.equal(report.layers.find(layer => layer.name === 'organization').state, 'loaded');
});
