import test from 'node:test';
import assert from 'node:assert/strict';
import { route } from './native-route.mjs';
import { catalog } from '../application/routing/catalog.mjs';
import { validatePlaybookCatalog } from '../engine/contracts/catalog.mjs';

test('fallback targets must resolve and belong to a declared primary skill', () => {
  const copy = structuredClone(catalog);
  copy.entries.find(entry => entry.id === 'workflow:review').fallbackSkills = { 'unknown:primary': 'craft-delivery:review-code' };
  assert.throws(() => validatePlaybookCatalog(copy), /invalid skill fallback/);
  const targets = new Set(catalog.entries.flatMap(entry => [...entry.skills, ...entry.references]));
  targets.delete('craft-delivery:review-code');
  assert.throws(() => validatePlaybookCatalog(catalog, { availableTargets: targets }), /unresolved catalog target: craft-delivery:review-code/);
});
