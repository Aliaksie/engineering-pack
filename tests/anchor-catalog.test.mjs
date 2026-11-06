import assert from 'node:assert/strict';
import fs from 'node:fs';
import test from 'node:test';
import { buildCatalog, notice, parseAnchor, serialize } from '../tooling/anchors-import.mjs';

const read = file => fs.readFileSync(new URL(`../${file}`, import.meta.url), 'utf8');
const catalogText = read('engine/anchors/catalog.json');
const catalog = JSON.parse(catalogText);
const overlay = JSON.parse(read('engine/anchors/overlay.json'));
const offered = overlay.anchors.filter(anchor => anchor.tier !== 'refused').map(anchor => anchor.name);

const adoc = (title, attributes, body = 'Body text with :colon: lines that are not attributes.') =>
  `= ${title}\n${Object.entries(attributes).map(([key, value]) => `:${key}: ${value}`).join('\n')}\n\n${body}\n`;

test('the importer reads header attributes of English anchors only', () => {
  const files = {
    'gherkin.adoc': adoc('Gherkin', { categories: 'testing-quality', roles: 'qa-engineer, software-developer', related: 'bdd, missing-anchor', proponents: 'Aslak Hellesøy', tier: '3', definition: 'Gherkin is a syntax.' }),
    'gherkin.de.adoc': adoc('Gherkin', { definition: 'Deutsch.' }),
    '_template.adoc': adoc('Template', { definition: 'Placeholder.' }),
    'bdd.adoc': adoc('BDD', { categories: 'testing-quality', roles: 'qa-engineer', proponents: 'Dan North', definition: 'Behaviour-driven development.', advisory: 'Use with care' }),
    'notes.txt': 'not an anchor',
  };
  const built = buildCatalog('a'.repeat(40), files);
  assert.equal(built.source.commit, 'a'.repeat(40));
  assert.deepEqual(built.anchors.map(anchor => anchor.id), ['bdd', 'gherkin']);
  const [bdd, gherkin] = built.anchors;
  assert.deepEqual(gherkin.roles, ['qa-engineer', 'software-developer']);
  assert.deepEqual(gherkin.related, ['bdd'], 'related names outside the catalog are dropped');
  assert.equal(gherkin.catalogTier, 3);
  assert.equal(bdd.catalogTier, null, 'a missing tier stays unknown');
  assert.equal(bdd.advisory, 'Use with care');
  assert.equal(gherkin.definition, 'Gherkin is a syntax.');
  assert.ok(!JSON.stringify(built).includes('Body text'), 'anchor bodies are not imported');
  assert.throws(() => parseAnchor('x', 'no title\n'), /first line is not a title/);
  assert.throws(() => parseAnchor('x', '= X\n:roles: a\n'), /no definition/);
  assert.throws(() => parseAnchor('x', '= X\nstray\n'), /unexpected header line/);
});

test('the committed catalog is unedited importer output', () => {
  assert.equal(serialize(catalog), catalogText, 'catalog.json is not in the form the importer writes');
  assert.equal(read('engine/anchors/NOTICE'), notice(catalog.source.commit));
  assert.match(read('engine/anchors/LICENSE'), /Apache License\s+Version 2\.0/);
  const ids = catalog.anchors.map(anchor => anchor.id);
  assert.deepEqual(ids, ids.toSorted());
  for (const anchor of catalog.anchors) for (const id of anchor.related) assert.ok(ids.includes(id), `${anchor.id} → ${id}`);
});

test('schema and docs name exactly the anchors the overlay offers', () => {
  const schema = JSON.parse(read('schemas/project.schema.json'));
  const standard = JSON.stringify(schema);
  const enumMatch = [...standard.matchAll(/"anchor":\{"enum":(\[[^\]]*\])/g)].map(match => JSON.parse(match[1]));
  assert.equal(enumMatch.length, 1, 'one anchor enum in the project schema');
  assert.deepEqual(enumMatch[0], offered);
  const routing = read('docs/ROUTING.md');
  const listed = /- Anchors are (.*?)\. An unknown name/s.exec(routing)?.[1];
  assert.ok(listed, 'docs/ROUTING.md lists the anchors');
  assert.deepEqual([...listed.matchAll(/`([a-z0-9-]+)`/g)].map(match => match[1]), offered);
  for (const anchor of overlay.anchors.filter(entry => entry.catalog)) {
    assert.ok(catalog.anchors.some(entry => entry.id === anchor.catalog), `${anchor.name}: catalog id ${anchor.catalog}`);
  }
});
