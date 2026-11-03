import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { EXPORTS, SCENARIOS, listingEntry, measure, portable } from '../tooling/overhead.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const budget = JSON.parse(fs.readFileSync(path.join(root, 'tests/fixtures/overhead-budget.json'), 'utf8'));
// Prompts where Craft has nothing to add: any byte a hook returns is pure overhead.
const NO_ADVANTAGE = ['chat', 'general-question'];

test('every export stays within its overhead ceilings and is silent where Craft adds nothing', () => {
  assert.deepEqual(Object.keys(budget.hosts).sort(), [...EXPORTS].sort(), 'each export has ceilings');
  const report = measure(path.join(root, 'dist'), process.env.CRAFT_NATIVE_BINARY);
  const over = [], slack = [];
  for (const [host, measured] of Object.entries(report)) {
    const ceiling = budget.hosts[host];
    const pairs = [['always', measured.always.bytes, ceiling.always], ['hooks', measured.hooks.bytes, ceiling.hooks],
      ...Object.entries(measured.guidance).map(([intent, bytes]) => [`guidance.${intent}`, bytes, ceiling.guidance[intent]])];
    for (const [name, value, limit] of pairs) {
      assert.equal(typeof limit, 'number', `${host} ${name} has a ceiling`);
      if (value > limit) over.push(`${host} ${name}: ${value} > ${limit}`);
      else if (value < limit * 0.9) slack.push(`${host} ${name}: ${value} (ceiling ${limit})`);
    }
    for (const id of NO_ADVANTAGE) assert.equal(measured.hooks.scenarios[id] ?? 0, 0, `${host} ${id} adds context`);
  }
  if (slack.length) console.log(`# overhead well below ceiling, lower it: ${slack.join('; ')}`);
  assert.deepEqual(over, [], 'overhead above its ceiling: reduce it, or raise the ceiling with the reason');
});

test('listing entries follow each host table and machine paths count as placeholders', () => {
  const entry = { pkg: 'craft-engineering', dir: 'foundations', name: 'foundations', description: 'Decide.' };
  assert.equal(listingEntry('claude', entry), '- craft-engineering:foundations: Decide.\n');
  assert.equal(listingEntry('codex', entry), '- craft-engineering:foundations: Decide. (file: r0/foundations/SKILL.md)\n');
  for (const host of EXPORTS) assert.match(listingEntry(host, entry), /^- \S+: Decide\./);
  assert.equal(portable('{"run":["/opt/x/craft","--root","C:\\\\work"]}', { '/opt/x/craft': '<craft>', 'C:\\work': '<root>' }),
    '{"run":["<craft>","--root","<root>"]}');
  assert.ok(SCENARIOS.some(({ id }) => NO_ADVANTAGE.includes(id)));
});
