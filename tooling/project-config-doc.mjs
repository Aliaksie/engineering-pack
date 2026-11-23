import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

// docs/PROJECT-CONFIG.md and schemas/layer.schema.json are generated from
// schemas/project.schema.json, so every field the schema accepts is documented once, with the
// same description editors show.
const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
export const SCHEMA = 'schemas/project.schema.json';
export const DOC = 'docs/PROJECT-CONFIG.md';
const SCHEMA_URL = 'https://raw.githubusercontent.com/Aliaksie/engineering-pack/main/schemas/project.schema.json';

// schemas/layer.schema.json is the same schema narrowed to what an organization or user file
// may set (engine `layers::FIELDS`): nothing that names one repository's files or modules.
export const LAYER_SCHEMA = 'schemas/layer.schema.json';
export const LAYER_FIELDS = ['$schema', 'formatVersion', 'routing', 'checkpoint', 'workspace', 'efficiency',
  'standards', 'anchors', 'skills', 'safety', 'execution', 'review'];
const PROJECT_ONLY = [['standards', 'items', 'file'], ['standards', 'items', 'folder'],
  ['standards', 'items', 'scope', 'modules'], ['anchors', 'catalogs'], ['skills', 'declared']];

export function layerSchema(schema) {
  const layer = structuredClone(schema);
  delete layer.$defs;
  delete layer.required;
  layer.description = 'A configuration layer: an organization file (CRAFT_ORGANIZATION_CONFIG, the managed '
    + 'location, or a project\'s committed .craft/organization.json) or a user file '
    + '(~/.config/craft/preferences.json). Every project inherits it and overrides only what differs; '
    + 'fields that name one repository\'s files or modules belong in the project file.';
  layer.properties = Object.fromEntries(LAYER_FIELDS.map(name => [name, layer.properties[name]]));
  for (const at of PROJECT_ONLY) {
    let node = layer.properties[at[0]];
    for (const step of at.slice(1, -1)) node = step === 'items' ? node.items : node.properties[step];
    delete node.properties[at.at(-1)];
  }
  layer.properties.standards.items.description = 'One standard: an `id` and one source (`anchor`, `checklist` or `run`).';
  return layer;
}

const json = value => `${JSON.stringify(value, null, 2)}\n`;

const cell = text => String(text).replaceAll('|', '\\|').replaceAll('\n', ' ');
const code = value => `\`${typeof value === 'string' ? value : JSON.stringify(value)}\``;

function resolve(schema, node) {
  return node?.$ref ? schema.$defs[node.$ref.replace('#/$defs/', '')] : node;
}

function range(node) {
  const low = node.minimum ?? node.exclusiveMinimum;
  const high = (node.maximum ?? node.exclusiveMaximum) >= Number.MAX_SAFE_INTEGER ? undefined : node.maximum ?? node.exclusiveMaximum;
  if (low === undefined && high === undefined) return '';
  return ` ${low ?? ''}–${high ?? ''}`;
}

function type(node) {
  if (node.const !== undefined) return code(node.const);
  if (node.enum) return node.enum.map(code).join(', ');
  if (node.type === 'array') {
    const items = node.items ?? {};
    const of = items.enum ? `of ${items.enum.map(code).join(', ')}` : items.type === 'object' ? 'of objects' : `of ${items.type ?? 'values'}`;
    return `array ${of}`;
  }
  if (node.type === 'object' && node.additionalProperties && typeof node.additionalProperties === 'object' && !node.properties) return 'map';
  return `${node.type ?? 'value'}${range(node)}`;
}

// Settings the schema still accepts, so existing files keep validating in an editor, but the
// reference leaves out: no guidance offers them and no recorded session set them.
export const HIDDEN = ['efficiency.attempts', 'efficiency.review', 'efficiency.delegation'];

function rows(schema, node, prefix, out) {
  for (const [name, raw] of Object.entries(node.properties ?? {})) {
    const field = resolve(schema, raw);
    const key = `${prefix}${name}`;
    if (HIDDEN.includes(key)) continue;
    if (raw.$ref) { out.push([key, 'object', '', raw.description ?? field.description]); continue; }
    out.push([key, type(field), field.default === undefined ? '' : code(field.default), field.description ?? '']);
    if (field.properties) rows(schema, field, `${key}.`, out);
    if (field.items?.properties) rows(schema, field.items, `${key}[].`, out);
    const entry = field.additionalProperties;
    if (entry && typeof entry === 'object' && entry.properties) rows(schema, entry, `${key}.<name>.`, out);
  }
  return out;
}

function table(list) {
  return ['| Field | Type | Default | Description |', '|---|---|---|---|',
    ...list.map(([key, kind, dflt, text]) => `| \`${key}\` | ${cell(kind)} | ${cell(dflt)} | ${cell(text)} |`)].join('\n');
}

export function render(schema) {
  const sections = Object.entries(schema.properties).filter(([name]) => name !== '$schema').map(([name, raw]) => {
    const field = resolve(schema, raw);
    const nested = rows(schema, { properties: { [name]: raw } }, '', []).slice(1);
    return [`## \`${name}\``, '', `${type(field)}${field.default === undefined ? '' : `, default ${code(field.default)}`}. ${field.description ?? ''}`,
      ...(nested.length ? ['', table(nested)] : [])].join('\n');
  });
  const service = schema.$defs.service;
  const bindings = service.oneOf.map(option => [`### ${option.description.replace(/\.$/, '')}`, '',
    table(rows(schema, option, '', []))].join('\n'));
  return [
    '# Project configuration',
    '',
    `<!-- Generated from ${SCHEMA} by \`npm run docs:config\`. Edit the schema, not this file. -->`,
    '',
    schema.description,
    '',
    '[Routing](ROUTING.md#project-standards) shows standards, anchors and skills by example;',
    '`examples/project/.craft/project.json` adds modules, services and execution.',
    '`craft settings --root <project>` prints the effective values with the origin of each.',
    '',
    'For completion and hover help in an editor, link the schema from the file:',
    '',
    '```json',
    `{ "$schema": "${SCHEMA_URL}", "formatVersion": 1 }`,
    '```',
    '',
    'An organization file and a user file can declare a baseline every project inherits:',
    '`routing`, `checkpoint`, `workspace`, `efficiency`, `standards`, `anchors`, `skills.disables`,',
    '`safety`, `execution` and `review`. The project overrides only what differs and',
    '`craft config` shows which layer won; see [configuration layers](SESSION.md#configuration-layers).',
    `A layer file links \`${LAYER_SCHEMA}\`, this schema without the fields that name one repository.`,
    '`craft config --sync --apply` commits the organization file as `.craft/organization.json`, the',
    'organization layer on machines without that file, such as CI runners.',
    'Task overrides take the switch sections only (`routing`, `checkpoint`, `workspace`,',
    '`efficiency`).',
    '',
    sections.join('\n\n'),
    '',
    '## Service bindings',
    '',
    service.description,
    '',
    bindings.join('\n\n'),
    '',
  ].join('\n');
}

function main(argv) {
  const schema = JSON.parse(fs.readFileSync(path.join(repository, SCHEMA), 'utf8'));
  const outputs = [[DOC, render(schema)], [LAYER_SCHEMA, json(layerSchema(schema))]];
  for (const [file, expected] of outputs) {
    const target = path.join(repository, file);
    if (argv.includes('--check')) {
      const current = fs.existsSync(target) ? fs.readFileSync(target, 'utf8').replace(/\r\n/g, '\n') : '';
      if (current !== expected) {
        console.error(`${file} is out of date; run npm run docs:config`);
        process.exitCode = 1;
      }
      continue;
    }
    fs.writeFileSync(target, expected);
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main(process.argv.slice(2));
