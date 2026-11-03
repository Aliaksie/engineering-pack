import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { aggregate, commandReads, exploration, markdown, taskMetadata, trials } from '../tooling/eval-summary.mjs';

const script = fileURLToPath(new URL('../tooling/eval-summary.mjs', import.meta.url));

// Codex code mode: one `exec` call that runs several tools.
const codeMode = (...tools) => ({ function_name: 'exec', arguments: { input: tools.map(tool =>
  tool.cmd ? `text(await tools.exec_command({cmd:${JSON.stringify(tool.cmd)},"max_output_tokens":3000}));`
    : `text(await tools.apply_patch(${JSON.stringify(tool.patch)}));`).join('\n') } });
const step = (at, calls, prompt = 1000, cached = 0) => ({
  source: 'agent', timestamp: `2026-11-03T10:00:${String(at).padStart(2, '0')}.000Z`,
  metrics: { prompt_tokens: prompt, cached_tokens: cached }, tool_calls: calls,
});
const trajectory = steps => ({ steps: [
  { source: 'system', timestamp: '2026-11-03T10:00:00.000Z', message: 'skills list' },
  { source: 'user', timestamp: '2026-11-03T10:00:00.000Z', message: '<environment_context>cwd</environment_context>' },
  { source: 'user', timestamp: '2026-11-03T10:00:01.000Z', message: 'Who calls applyDiscount?' },
  { source: 'system', timestamp: '2026-11-03T10:00:01.000Z', message: 'Routing hints' },
  ...steps,
] });
const answer = (file = '/app/ANSWER.md') => ({ patch: `*** Begin Patch\n*** Add File: ${file}\n+src/A.java#run\n*** End Patch` });

test('exploration stops at the first write of the answer file and keeps whole-trial totals', () => {
  const result = exploration(trajectory([
    step(5, [codeMode({ cmd: 'rg -n applyDiscount .' }, { cmd: 'cat /installed-agent/codex-home/plugins/cache/pack/x/0.0.3/SKILL.md' })], 1000, 400),
    step(9, [codeMode({ cmd: 'cat src/{A,B}.java && git status' })], 2000, 1500),
    step(12, [codeMode(answer())], 3000, 2500),
    step(14, [codeMode({ cmd: 'cat ANSWER.md' })], 3100, 3000),
  ]), 'ANSWER.md');
  assert.deepEqual(result.toAnswer, {
    answered: true, steps: 3, toolCalls: 3, commands: 3, searches: 1, fileReads: 3, packageReads: 1,
    inputTokens: 6000, cachedTokens: 4400, seconds: 11,
  });
  assert.equal(result.total.steps, 4);
  assert.equal(result.total.fileReads, 4);
  assert.equal(result.total.injectedBytes, Buffer.byteLength('skills list') + Buffer.byteLength('Routing hints'));
});

test('an answer written by a shell redirect or a Claude Write counts; a read of it does not', () => {
  const shell = exploration(trajectory([
    step(3, [{ function_name: 'exec_command', arguments: { cmd: 'cat ANSWER.md' } }]),
    step(4, [{ function_name: 'exec_command', arguments: { cmd: "printf 'x' > ANSWER.md" } }]),
  ]), 'ANSWER.md');
  assert.equal(shell.toAnswer.steps, 2);
  const claude = exploration(trajectory([
    step(3, [{ function_name: 'Grep', arguments: { pattern: 'x' } }, { function_name: 'Read', arguments: { file_path: '/app/src/A.java' } }]),
    step(4, [{ function_name: 'Write', arguments: { file_path: '/app/ANSWER.md' } }]),
  ]), 'ANSWER.md');
  assert.equal(claude.toAnswer.searches, 1);
  assert.equal(claude.toAnswer.fileReads, 1);
  assert.equal(claude.toAnswer.toolCalls, 3);
});

test('a trial that never writes its answer file is unanswered, and one without metrics has unknown tokens', () => {
  const result = exploration({ steps: [{ source: 'agent', timestamp: '2026-11-03T10:00:01.000Z', tool_calls: [] }] }, 'ANSWER.md');
  assert.deepEqual(result.toAnswer, { answered: false });
  assert.equal(result.total.inputTokens, null);
  assert.equal(result.total.seconds, null);
});

test('command reads skip flags, scripts and redirects and expand braces', () => {
  assert.deepEqual(commandReads("sed -n '1,80p' src/a.js | head -5 && nl -ba docs/b.md; cat src/{c,d}.ts 2>/dev/null"),
    ['src/a.js', 'docs/b.md', 'src/c.ts', 'src/d.ts']);
  assert.deepEqual(commandReads('rg -n foo . && git log -1'), []);
});

function job(trialsByArm) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-eval-summary-'));
  const task = path.join(root, 'evals', 'tasks', 'lookup');
  fs.mkdirSync(task, { recursive: true });
  fs.writeFileSync(path.join(task, 'task.toml'), '[task]\nname = "craft/lookup"\n\n[metadata]\nactor = "developer"\nanswer_file = "ANSWER.md"\n');
  const jobDir = path.join(root, 'job');
  fs.mkdirSync(jobDir);
  fs.writeFileSync(path.join(jobDir, 'result.json'), '{"id":"job"}');
  let index = 0;
  for (const [arm, runs] of Object.entries(trialsByArm)) {
    for (const { reward, calls } of runs) {
      const dir = path.join(jobDir, `lookup__${arm}${index++}`);
      fs.mkdirSync(path.join(dir, 'agent'), { recursive: true });
      fs.writeFileSync(path.join(dir, 'result.json'), JSON.stringify({
        trial_name: path.basename(dir), task_name: 'craft/lookup',
        config: { task: { path: 'evals/tasks/lookup' }, agent: { name: 'codex', kwargs: { config: `evals/codex/${arm}.toml` } } },
        agent_info: { name: 'codex', model_info: { name: 'm' } },
        agent_result: { n_input_tokens: 10, n_cache_tokens: 5, n_output_tokens: 1, cost_usd: 0.01 },
        verifier_result: { rewards: { reward, craft_events: arm === 'craft' ? 3 : 0 } },
      }));
      const steps = Array.from({ length: calls - 1 }, (_, i) => step(2 + i, [codeMode({ cmd: 'rg -n x .' })]));
      fs.writeFileSync(path.join(dir, 'agent', 'trajectory.json'), JSON.stringify(trajectory([...steps, step(2 + calls, [codeMode(answer())])])));
    }
  }
  return { root, jobDir };
}

test('aggregate takes medians per task and arm and compares the Craft arm with vanilla', () => {
  const { root, jobDir } = job({
    craft: [{ reward: 1, calls: 2 }, { reward: 1, calls: 3 }, { reward: 0, calls: 10 }],
    vanilla: [{ reward: 1, calls: 4 }, { reward: 1, calls: 6 }, { reward: 1, calls: 6 }],
  });
  try {
    const entries = trials(jobDir, { root });
    assert.equal(entries.length, 6);
    const rows = aggregate(entries);
    const craft = rows.find(row => row.arm === 'craft');
    assert.equal(craft.n, 3);
    assert.equal(craft.correct, 2);
    assert.equal(craft.answered, 3);
    assert.equal(craft.basis, 'first answer');
    assert.equal(craft.toolCalls, 3);
    assert.deepEqual(craft.versus, { arm: 'vanilla', deciding: true, ratio: { toolCalls: 0.5, inputTokens: 0.5, seconds: 0.57, injectedBytes: 1 } });
    assert.equal(rows.find(row => row.arm === 'vanilla').versus, undefined);
    assert.match(markdown(entries, rows), /\| lookup \| craft \| 3 \| 2\/3 \| 3\/3 \| first answer \| 3 \|.*\| 0\.5× \/ 0\.5× \/ 0\.57× \|/);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});

test('fewer than three attempts per arm is shown as not deciding', () => {
  const { root, jobDir } = job({ craft: [{ reward: 1, calls: 2 }], vanilla: [{ reward: 1, calls: 4 }] });
  try {
    const rows = aggregate(trials(jobDir, { root }));
    assert.equal(rows.find(row => row.arm === 'craft').versus.deciding, false);
    const run = spawnSync(process.execPath, [script, jobDir, '--root', root, '--title', 'compare'], { encoding: 'utf8' });
    assert.equal(run.status, 0, run.stderr);
    assert.match(run.stdout, /### compare/);
    assert.match(run.stdout, /not deciding/);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});

test('task metadata reads flat strings from the metadata table only', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-task-toml-'));
  try {
    const file = path.join(dir, 'task.toml');
    fs.writeFileSync(file, '[task]\nanswer_file = "WRONG.md"\n[metadata]\nanswer_file = "ANSWER.md" # comment\nscenario = "a \\"quoted\\" one"\n[agent]\nuser = "node"\n');
    assert.deepEqual(taskMetadata(file), { answer_file: 'ANSWER.md', scenario: 'a "quoted" one' });
    assert.deepEqual(taskMetadata(path.join(dir, 'missing.toml')), {});
  } finally { fs.rmSync(dir, { recursive: true, force: true }); }
});
