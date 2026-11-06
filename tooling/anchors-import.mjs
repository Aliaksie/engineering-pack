#!/usr/bin/env node
// Generates the anchor catalog `craft` embeds from a checkout of the Semantic Anchors catalog
// (Apache-2.0). Only the header attributes of the English anchor files are read; the anchor
// bodies and the upstream scripts are not used. The output is reviewed and committed; never
// edit it by hand. `--check` fails when the committed files differ from what the checkout
// produces.
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const REPOSITORY = 'https://github.com/LLM-Coding/Semantic-Anchors';
const ANCHORS_DIR = 'docs/anchors';
const LISTS = ['categories', 'roles', 'related', 'proponents'];

const split = value => (value ?? '').split(',').map(item => item.trim()).filter(Boolean);

/** One anchor from an `.adoc` file: the `= Title` line, then `:name: value` lines up to the first blank line. */
export function parseAnchor(id, text) {
  const lines = text.split(/\r?\n/);
  if (!lines[0]?.startsWith('= ')) throw new Error(`${id}: first line is not a title`);
  const attributes = {};
  for (const line of lines.slice(1)) {
    if (!line.trim()) break;
    const match = /^:([a-z-]+):\s*(.*)$/.exec(line);
    if (!match) throw new Error(`${id}: unexpected header line: ${line}`);
    attributes[match[1]] = match[2].trim();
  }
  if (!attributes.definition) throw new Error(`${id}: no definition`);
  const anchor = { id, title: lines[0].slice(2).trim(), definition: attributes.definition };
  for (const list of LISTS) anchor[list] = split(attributes[list]);
  const tier = Number(attributes.tier);
  anchor.catalogTier = [1, 2, 3].includes(tier) ? tier : null;
  if (attributes.advisory) anchor.advisory = attributes.advisory;
  return anchor;
}

/** The catalog for `files` ({name → text}): English anchors only, in id order. */
export function buildCatalog(commit, files) {
  const anchors = Object.keys(files)
    .filter(name => name.endsWith('.adoc') && name !== '_template.adoc' && !/\.[a-z]{2}\.adoc$/.test(name))
    .sort()
    .map(name => parseAnchor(name.slice(0, -'.adoc'.length), files[name]));
  const ids = new Set(anchors.map(anchor => anchor.id));
  for (const anchor of anchors) anchor.related = anchor.related.filter(id => ids.has(id));
  return { source: { repository: REPOSITORY, commit, path: ANCHORS_DIR, license: 'Apache-2.0' }, anchors };
}

/** One anchor per line, so a refresh reviews as a per-anchor diff. */
export function serialize(catalog) {
  const rows = catalog.anchors.map(anchor => `    ${JSON.stringify(anchor)}`).join(',\n');
  return `{\n  "source": ${JSON.stringify(catalog.source)},\n  "anchors": [\n${rows}\n  ]\n}\n`;
}

export function notice(commit) {
  return `Craft embeds anchor catalog data derived from Semantic Anchors
${REPOSITORY}
Copyright the Semantic Anchors project and its contributors.
Licensed under the Apache License, Version 2.0; see LICENSE in this directory.

Source commit: ${commit}
Changes: catalog.json holds the header attributes (title, definition, categories, roles,
related, proponents, tier, advisory) of the English files in ${ANCHORS_DIR}, converted to JSON;
related names outside the catalog are dropped. Anchor bodies, translations and upstream code
are not included. overlay.json is Craft's own work and not part of the upstream catalog.
`;
}

function readCheckout(checkout) {
  const commit = execFileSync('git', ['-C', checkout, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim();
  const directory = path.join(checkout, ANCHORS_DIR);
  const files = Object.fromEntries(fs.readdirSync(directory).map(name => [name, fs.readFileSync(path.join(directory, name), 'utf8')]));
  return { commit, files, license: fs.readFileSync(path.join(checkout, 'LICENSE'), 'utf8') };
}

function main(argv) {
  const check = argv.includes('--check');
  const checkout = argv.find(arg => !arg.startsWith('--'));
  if (!checkout) throw new Error('usage: anchors-import.mjs <Semantic-Anchors checkout> [--check]');
  const target = fileURLToPath(new URL('../engine/anchors/', import.meta.url));
  const { commit, files, license } = readCheckout(checkout);
  const outputs = { 'catalog.json': serialize(buildCatalog(commit, files)), NOTICE: notice(commit), LICENSE: license };
  if (check) {
    const stale = Object.entries(outputs).filter(([name, text]) => {
      const file = path.join(target, name);
      return !fs.existsSync(file) || fs.readFileSync(file, 'utf8') !== text;
    }).map(([name]) => name);
    if (stale.length) throw new Error(`engine/anchors differs from ${commit}: ${stale.join(', ')}`);
    console.log(`engine/anchors matches ${commit}`);
    return;
  }
  fs.mkdirSync(target, { recursive: true });
  for (const [name, text] of Object.entries(outputs)) fs.writeFileSync(path.join(target, name), text);
  console.log(`engine/anchors written from ${commit}`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
