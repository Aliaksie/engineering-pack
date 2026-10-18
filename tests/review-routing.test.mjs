import test from 'node:test';
import assert from 'node:assert/strict';
import { route } from '../application/routing/select.mjs';
import { catalog } from '../application/routing/catalog.mjs';
import { validatePlaybookCatalog } from '../engine/contracts/catalog.mjs';

test('review selects only an explicitly installed fallback and explains the substitution', () => {
  const primary = 'craft-review:review-change';
  const fallback = 'craft-delivery:review-code';
  for (const availableSkills of [undefined, [primary], [primary, fallback], []]) {
    const result = route({ intent: 'review', availableSkills });
    assert.ok(result.skills.includes(primary));
    assert.ok(!result.skills.includes(fallback));
    if (availableSkills?.length === 0) assert.ok(result.missing.includes(`skill not installed: ${primary}`));
  }
  const result = route({ intent: 'review', availableSkills: [fallback] });
  assert.ok(result.skills.includes(fallback));
  assert.ok(!result.skills.includes(primary));
  assert.deepEqual(result.loading.selections.find(entry => entry.id === 'workflow:review').substitutions,
    [{ unavailable: primary, selected: fallback, reason: 'Declared fallback is installed; primary skill is unavailable.' }]);
  assert.equal(result.authorization, 'route-only; host permissions and original user scope still apply');
});

test('fallback targets must resolve and belong to a declared primary skill', () => {
  const copy = structuredClone(catalog);
  copy.entries.find(entry => entry.id === 'workflow:review').fallbackSkills = { 'unknown:primary': 'craft-delivery:review-code' };
  assert.throws(() => validatePlaybookCatalog(copy), /invalid skill fallback/);
  const targets = new Set(catalog.entries.flatMap(entry => [...entry.skills, ...entry.references]));
  targets.delete('craft-delivery:review-code');
  assert.throws(() => validatePlaybookCatalog(catalog, { availableTargets: targets }), /unresolved catalog target: craft-delivery:review-code/);
});
