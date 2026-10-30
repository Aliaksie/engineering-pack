import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import test from 'node:test';
import { spawnSync } from 'node:child_process';
import { run } from './craft.mjs';
import { deliverHook } from './native-hook.mjs';

const corpus = name => JSON.parse(fs.readFileSync(new URL(`./fixtures/decisions/${name}.json`, import.meta.url), 'utf8'));
const scores = [];
const gaps = [];

// A changed decision fails until `current` is updated; the distance to `expected` is reported,
// not enforced. `observe` returns the decision for one case.
function replay(t, title, cases, observe) {
  let matched = 0;
  for (const { id, expected, current } of cases) {
    const observed = observe(id);
    assert.deepEqual(observed, current, `${id}: decision changed; update current deliberately`);
    // The prompt corpus also records the skills a hint names; expectations name the intent only.
    const relevant = Object.fromEntries(Object.keys(expected).map(key => [key, observed[key]]));
    if (JSON.stringify(relevant) === JSON.stringify(expected)) {
      matched++;
      continue;
    }
    gaps.push(`| ${title} | ${id} | ${JSON.stringify(expected)} | ${JSON.stringify(relevant)} |`);
    t.diagnostic(`gap ${id}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(relevant)}`);
  }
  t.diagnostic(`${matched}/${cases.length} ${title} decisions as expected`);
  scores.push(`${title}: ${matched}/${cases.length} as expected`);
}

// A committed repository with one Java class whose method has a caller.
function repository(t) {
  const root = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-decision-replay-')));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.mkdirSync(path.join(root, 'src'));
  fs.writeFileSync(path.join(root, 'src/Order.java'), 'class Order {\n  int total() { return applyDiscount(1); }\n  int applyDiscount(int v) { return v; }\n}\n');
  const git = (...args) => assert.equal(spawnSync('git', ['-C', root, '-c', 'core.hooksPath=/dev/null', '-c', 'commit.gpgsign=false',
    '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test', ...args]).status, 0);
  git('init', '-q');
  git('add', '.'); git('commit', '-qm', 'fixture');
  return root;
}

test('prompt routing replay', t => {
  const root = repository(t);
  const cases = corpus('prompt-routing').cases;
  const prompts = new Map(cases.map(({ id, prompt }) => [id, prompt]));
  // A separate session per case: an identical hint is suppressed within one session.
  replay(t, 'prompt routing', cases, id => {
    const data = deliverHook(root, 'before-prompt', { prompt: prompts.get(id), session: id, host: 'claude' }).context?.data;
    return { intent: data?.intent ?? null, skills: data?.skills ?? [] };
  });
});

test('session situation replay', t => {
  const intent = (root, session) => ({
    intent: deliverHook(root, 'before-tool', { tool: 'edit', file: path.join(root, 'src/Order.java'), session, host: 'claude' })
      .context?.data?.intent ?? null,
  });
  const lookup = (root, project) => {
    if (project) {
      fs.mkdirSync(path.join(root, '.craft'));
      fs.writeFileSync(path.join(root, '.craft/project.json'), JSON.stringify(project));
    }
    const { answeredBy, reason, complete } = run(['locate', '--root', root, '--callers', 'applyDiscount', '--path', 'src']).route;
    return { answeredBy, reason, complete };
  };
  const enabled = { formatVersion: 1, evidence: { enabled: true } };
  const situations = {
    'edit-first': root => intent(root, 'edit'),
    'edit-repeated': root => (intent(root, 'edit'), intent(root, 'edit')),
    'start-unfinished-task': root => {
      run(['task-start', '--root', root, '--session', 'first', '--task', 'fix-discount', '--goal', 'Correct the discount', '--file', 'src/Order.java']);
      const line = deliverHook(root, 'session-start', { session: 'second', host: 'claude' }).session
        .find(text => text.startsWith('Unfinished tasks of earlier sessions'));
      return { offered: line ? JSON.parse(line.slice(line.indexOf(': [') + 2)).map(task => task.task) : [] };
    },
    'lookup-default': root => lookup(root),
    'lookup-enabled': root => lookup(root, enabled),
    'lookup-unindexed': root => {
      fs.writeFileSync(path.join(root, 'src/cart.mjs'), 'export const total = items => applyDiscount(items.length);\n');
      return lookup(root, enabled);
    },
  };
  replay(t, 'session situation', corpus('situations').cases, id => {
    assert.ok(situations[id], `${id}: no situation defined`);
    return situations[id](repository(t));
  });
});

// One CI matrix entry publishes the scorecard to the job summary.
test.after(() => {
  if (!process.env.GITHUB_STEP_SUMMARY || process.env.CRAFT_DECISION_SUMMARY !== 'true') return;
  fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, ['## Decision replay', '', ...scores.map(score => `- ${score}`), '',
    '| Corpus | Case | Expected | Current |', '|---|---|---|---|', ...gaps, ''].join('\n'));
});
