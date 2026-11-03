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
    // The prompt corpus also records the skills a hint names; expectations name the intent and
    // optionally the leading workflow, the first skill the hint names.
    const derived = { ...observed, workflow: observed.skills?.[0] ?? null };
    const relevant = Object.fromEntries(Object.keys(expected).map(key => [key, derived[key]]));
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
    // A ready lookup names no skill; a prompt the hook answered records the question it answered.
    return { intent: data?.intent ?? null, skills: data?.skills ?? [], ...(data?.answer && { answer: data.answer.question }) };
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
  // Hook events in one session: the hint each one delivers and how often routing ran.
  const sequence = (root, host, steps) => {
    const hints = steps.map(([lifecycle, value]) => {
      const input = lifecycle === 'before-prompt' ? { prompt: value } : { tool: 'edit', file: path.join(root, value) };
      return deliverHook(root, lifecycle, { ...input, session: 'sequence', host }).context?.data?.intent ?? null;
    });
    const log = path.join(root, '.craft/state/telemetry/events.jsonl');
    const events = fs.existsSync(log) ? fs.readFileSync(log, 'utf8').trim().split('\n').map(line => JSON.parse(line)) : [];
    return { hints, routings: events.filter(event => event.name === 'routing.completed').length };
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
    'plan-file-in-plan-session': root => sequence(root, 'claude', [
      ['before-prompt', 'Plan how to cap the discount at 50%. Write the plan to PLAN.md.'],
      ['before-tool', 'PLAN.md'], ['after-tool', 'PLAN.md']]),
    'answer-file-in-lookup-session': root => sequence(root, 'claude', [
      ['before-prompt', 'Which methods call applyDiscount? Write them to ANSWER.md.'],
      ['before-tool', 'ANSWER.md'], ['after-tool', 'ANSWER.md']]),
    'edit-routes-once-per-tool-call': root => sequence(root, 'claude', [
      ['before-tool', 'src/Order.java'], ['after-tool', 'src/Order.java']]),
    'implement-session-routes-each-new-file': root => sequence(root, 'claude', [
      ['before-prompt', 'Fix the rounding in applyDiscount.'],
      ['before-tool', 'src/Order.java'], ['after-tool', 'src/Order.java'], ['before-tool', 'src/Order.java']]),
    'edit-routes-after-tool-without-before-tool-context': root => sequence(root, 'cursor', [
      ['before-tool', 'src/Order.java'], ['after-tool', 'src/Order.java']]),
    'plan-file-is-planning-evidence': root => {
      const write = text => {
        deliverHook(root, 'before-tool', { tool: 'edit', file: path.join(root, 'PLAN.md'), session: 'planner', host: 'claude' });
        fs.writeFileSync(path.join(root, 'PLAN.md'), text);
        deliverHook(root, 'after-tool', { tool: 'edit', file: path.join(root, 'PLAN.md'), session: 'planner', host: 'claude' });
      };
      deliverHook(root, 'before-prompt', { prompt: 'Plan how to cap the discount at 50%. Write the plan to PLAN.md.', session: 'planner', host: 'claude' });
      write('# Cap the discount\n');
      write('# Cap the discount\n\n1. Clamp in applyDiscount.\n');
      const planning = (run(['session', '--root', root, '--session', 'planner']).planning ?? [])
        .map(({ path: file, revision, current, intent }) => ({ path: file, revision, current, intent }));
      const line = deliverHook(root, 'session-start', { session: 'next', host: 'claude' }).session
        .find(text => text.startsWith('Planning evidence of earlier sessions'));
      const offered = line ? JSON.parse(line.slice(line.indexOf(': [') + 2)).map(entry => entry.path) : [];
      return { planning, offered, workPlans: fs.existsSync(path.join(root, '.craft/state/plans')) };
    },
    // Codex writes files from its shell tool: no edit event, only the turn end sees the file.
    // A file already changed before the plan was requested is not evidence.
    'plan-written-by-shell-is-planning-evidence': root => {
      fs.writeFileSync(path.join(root, 'notes.md'), 'earlier work\n');
      const earlier = new Date(Date.now() - 60_000);
      fs.utimesSync(path.join(root, 'notes.md'), earlier, earlier);
      deliverHook(root, 'before-prompt', { prompt: 'Plan how to cap the discount at 50%. Write the plan to PLAN.md.', session: 'planner', host: 'codex' });
      deliverHook(root, 'before-tool', { tool: 'shell', session: 'planner', host: 'codex' });
      fs.writeFileSync(path.join(root, 'PLAN.md'), '# Cap the discount\n');
      deliverHook(root, 'after-tool', { tool: 'shell', session: 'planner', host: 'codex' });
      deliverHook(root, 'turn-end', { session: 'planner', host: 'codex' });
      const planning = (run(['session', '--root', root, '--session', 'planner']).planning ?? [])
        .map(({ path: file, revision, current, intent }) => ({ path: file, revision, current, intent }));
      return { planning, workPlans: fs.existsSync(path.join(root, '.craft/state/plans')) };
    },
    'lookup-default': root => lookup(root),
    'lookup-enabled': root => lookup(root, enabled),
    'lookup-unindexed': root => {
      fs.writeFileSync(path.join(root, 'src/cart.kt'), 'fun total(items: List<Int>) = applyDiscount(items.size)\n');
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
