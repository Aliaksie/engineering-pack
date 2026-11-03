#!/usr/bin/env node
// Summarizes one Harbor job: every trial's outcome and exploration cost, and per task and arm
// the medians across attempts with the Craft arm relative to the vanilla arm. Read-only; it
// does not run or judge a model. Exploration is counted until the agent first writes the
// task's answer file (`answer_file` in task.toml metadata), so "cost to the first answer"
// stays deterministic; correctness comes from the hidden verifier. A value the trajectory
// does not carry stays unknown (null), never 0.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { SEARCH } from './session-audit.mjs';

// Installed plugin or package files: guidance the agent read instead of the repository.
const PACKAGE_FILE = /(?:^|\/)(?:plugins\/cache|\.claude\/plugins|opt\/craft)\//;
const READERS = new Set(['cat', 'head', 'tail', 'nl', 'less', 'more', 'bat', 'sed', 'awk']);
const MIN_DECIDING_ATTEMPTS = 3;
const BASELINE = 'vanilla';

const readJson = file => { try { return JSON.parse(fs.readFileSync(file, 'utf8')); } catch { return null; } };
const median = values => {
  const sorted = values.filter(Number.isFinite).sort((a, b) => a - b);
  if (!sorted.length) return null;
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
};
const seconds = (from, to) => {
  const span = (Date.parse(to) - Date.parse(from)) / 1000;
  return Number.isFinite(span) ? Math.round(span * 10) / 10 : null;
};

// `[metadata] key = "value"` from a task.toml; enough for the flat strings tasks declare.
export function taskMetadata(file) {
  let text;
  try { text = fs.readFileSync(file, 'utf8'); } catch { return {}; }
  const metadata = {};
  let section = null;
  for (const line of text.split('\n')) {
    const header = line.match(/^\s*\[([^\]]+)\]\s*$/);
    if (header) { section = header[1].trim(); continue; }
    const pair = section === 'metadata' && line.match(/^\s*([A-Za-z0-9_-]+)\s*=\s*"((?:[^"\\]|\\.)*)"/);
    if (pair) metadata[pair[1]] = JSON.parse(`"${pair[2]}"`);
  }
  return metadata;
}

// Simple `{a,b}` brace expansion, as shells apply it to `cat src/{A,B}.java`.
const expand = word => {
  const match = word.match(/^(.*?)\{([^{}]*,[^{}]*)\}(.*)$/);
  return match ? match[2].split(',').flatMap(part => expand(match[1] + part + match[3])) : [word];
};

// Files a shell command reads with a pager or text tool; a heuristic, not a parser.
export function commandReads(command) {
  const reads = [];
  for (const segment of command.split(/&&|\|\||[;|\n]/)) {
    const words = segment.trim().match(/'[^']*'|"[^"]*"|\S+/g) ?? [];
    if (!READERS.has(words[0])) continue;
    let rest = words.slice(1);
    // sed and awk take a script first; only the arguments after it are files.
    if (words[0] === 'sed' || words[0] === 'awk') {
      rest = rest.filter(word => !word.startsWith('-'));
      rest = rest.slice(1);
    }
    for (const word of rest) {
      if (word.startsWith('-') || /^[<>0-9]/.test(word) || /^['"]/.test(word) && !/[./]/.test(word)) continue;
      const unquoted = word.replace(/^['"]|['"]$/g, '');
      if (/[./]/.test(unquoted)) reads.push(...expand(unquoted));
    }
  }
  return reads;
}

const writesFile = (target, name) => target === name || target.endsWith(`/${name}`);
const shellWrites = (command, name) => new RegExp(`(?:>>?|\\btee(?:\\s+-a)?)\\s*['"]?(?:\\S*/)?${name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}\\b`).test(command);
const patchWrites = (patch, name) => [...String(patch).matchAll(/\*\*\* (?:Add|Update) File: (\S+)/g)].some(([, file]) => writesFile(file, name));

// Every tool call reduced to the shell commands, file reads and writes it performed. Covers
// Codex code mode (one `exec` call running several tools), plain Codex tools and Claude
// Code's tools; anything else counts as a call with no recognized effect.
export function operations(call) {
  const args = call.arguments ?? {};
  const name = call.function_name;
  const ops = [];
  const command = text => ops.push({ kind: 'command', text: Array.isArray(text) ? text.join(' ') : String(text) });
  if (name === 'exec' && typeof args.input === 'string') {
    for (const [, literal] of args.input.matchAll(/exec_command\(\{\s*cmd:\s*("(?:[^"\\]|\\.)*")/g)) {
      try { command(JSON.parse(literal)); } catch {}
    }
    for (const [, literal] of args.input.matchAll(/apply_patch\(\s*("(?:[^"\\]|\\.)*")/g)) {
      try { ops.push({ kind: 'patch', text: JSON.parse(literal) }); } catch {}
    }
  } else if (['exec_command', 'shell', 'Bash', 'bash'].includes(name)) {
    command(args.cmd ?? args.command ?? '');
  } else if (name === 'apply_patch') {
    ops.push({ kind: 'patch', text: args.input ?? args.patch ?? '' });
  } else if (name === 'Read' && args.file_path) {
    ops.push({ kind: 'read', path: args.file_path });
  } else if (['Write', 'Edit', 'MultiEdit', 'NotebookEdit'].includes(name) && (args.file_path ?? args.notebook_path)) {
    ops.push({ kind: 'write', path: args.file_path ?? args.notebook_path });
  } else if (['Grep', 'Glob'].includes(name)) {
    ops.push({ kind: 'search' });
  }
  return ops;
}

function tally(calls) {
  const counts = { toolCalls: calls.length, commands: 0, searches: 0, fileReads: 0, packageReads: 0 };
  for (const call of calls) {
    for (const op of operations(call)) {
      if (op.kind === 'command') {
        counts.commands++;
        if (SEARCH.test(op.text)) counts.searches++;
        const reads = commandReads(op.text);
        counts.fileReads += reads.length;
        counts.packageReads += reads.filter(file => PACKAGE_FILE.test(file)).length;
      } else if (op.kind === 'read') {
        counts.fileReads++;
        if (PACKAGE_FILE.test(op.path)) counts.packageReads++;
      } else if (op.kind === 'search') {
        counts.searches++;
      }
    }
  }
  return counts;
}

const answers = (call, name) => operations(call).some(op =>
  op.kind === 'command' ? shellWrites(op.text, name)
    : op.kind === 'patch' ? patchWrites(op.text, name)
      : op.kind === 'write' && writesFile(op.path, name));

// Exploration over the agent's steps, in total and until the first answer-file write.
export function exploration(trajectory, answerFile) {
  const steps = trajectory?.steps ?? [];
  const agent = steps.filter(step => step.source === 'agent');
  const prompt = steps.find(step => step.source === 'user' && !String(step.message ?? '').startsWith('<environment_context>'));
  const usage = list => {
    const known = list.filter(step => step.metrics);
    if (!known.length) return { inputTokens: null, cachedTokens: null };
    return {
      inputTokens: known.reduce((sum, step) => sum + (step.metrics.prompt_tokens ?? 0), 0),
      cachedTokens: known.reduce((sum, step) => sum + (step.metrics.cached_tokens ?? 0), 0),
    };
  };
  const measure = list => ({
    steps: list.length, ...tally(list.flatMap(step => step.tool_calls ?? [])), ...usage(list),
    seconds: prompt && list.length ? seconds(prompt.timestamp, list.at(-1).timestamp) : null,
  });
  const total = measure(agent);
  // Context injected outside the conversation: host instructions, skill lists, hook output.
  total.injectedBytes = steps.filter(step => step.source === 'system')
    .reduce((sum, step) => sum + Buffer.byteLength(typeof step.message === 'string' ? step.message : JSON.stringify(step.message ?? '')), 0);
  let toAnswer = null;
  if (answerFile) {
    const at = agent.findIndex(step => (step.tool_calls ?? []).some(call => answers(call, answerFile)));
    toAnswer = at < 0 ? { answered: false } : { answered: true, ...measure(agent.slice(0, at + 1)) };
  }
  return { total, toAnswer };
}

const armOf = result => {
  const config = result.config?.agent?.kwargs?.config;
  return config ? path.basename(config).replace(/\.toml$/, '') : result.agent_info?.name ?? result.config?.agent?.name ?? 'unknown';
};

export function trial(directory, { root = process.cwd() } = {}) {
  const result = readJson(path.join(directory, 'result.json'));
  if (!result?.trial_name) return null;
  const taskPath = result.config?.task?.path;
  const metadata = taskPath ? taskMetadata(path.resolve(root, taskPath, 'task.toml')) : {};
  const rewards = result.verifier_result?.rewards ?? {};
  const trajectory = readJson(path.join(directory, 'agent', 'trajectory.json'));
  const explored = trajectory ? exploration(trajectory, metadata.answer_file) : null;
  const agentResult = result.agent_result ?? {};
  return {
    task: result.task_name?.replace(/^craft\//, '') ?? path.basename(taskPath ?? ''),
    trial: result.trial_name,
    arm: armOf(result),
    model: result.agent_info?.model_info?.name ?? null,
    reward: Number.isFinite(rewards.reward) ? rewards.reward : null,
    answerFile: metadata.answer_file ?? null,
    tokens: { input: agentResult.n_input_tokens ?? null, cached: agentResult.n_cache_tokens ?? null, output: agentResult.n_output_tokens ?? null },
    costUsd: Number.isFinite(agentResult.cost_usd) ? Math.round(agentResult.cost_usd * 10000) / 10000 : null,
    agentSeconds: result.agent_execution?.started_at && result.agent_execution?.finished_at
      ? seconds(result.agent_execution.started_at, result.agent_execution.finished_at) : null,
    total: explored?.total ?? null,
    toAnswer: explored?.toAnswer ?? null,
    craft: {
      events: rewards.craft_events ?? null, lookups: rewards.craft_lookups ?? null,
      hookMs: rewards.craft_hook_ms_total ?? null, expectedSkill: rewards.expected_skill_selected ?? null,
      plan: rewards.craft_plan_created ?? null,
    },
    error: result.exception_info?.exception_type ?? null,
  };
}

export function trials(jobDirectory, options) {
  return fs.readdirSync(jobDirectory, { withFileTypes: true })
    .filter(entry => entry.isDirectory())
    .map(entry => trial(path.join(jobDirectory, entry.name), options))
    .filter(Boolean)
    .sort((a, b) => a.task.localeCompare(b.task) || a.arm.localeCompare(b.arm) || a.trial.localeCompare(b.trial));
}

// The exploration a comparison uses: to the first answer where the task has one, else total.
const basis = entry => entry.toAnswer?.answered ? entry.toAnswer : entry.total;
const MEASURES = ['toolCalls', 'commands', 'searches', 'fileReads', 'packageReads', 'inputTokens', 'seconds'];

export function aggregate(entries) {
  const groups = new Map();
  for (const entry of entries) {
    const key = `${entry.task}\u0000${entry.arm}`;
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(entry);
  }
  const rows = [...groups.values()].map(list => {
    const graded = list.filter(entry => entry.reward !== null);
    const row = {
      task: list[0].task, arm: list[0].arm, n: list.length,
      correct: graded.filter(entry => entry.reward >= 1).length, graded: graded.length,
      answered: list[0].answerFile ? list.filter(entry => entry.toAnswer?.answered).length : null,
      errors: list.filter(entry => entry.error).length,
      basis: list[0].answerFile ? 'first answer' : 'whole trial',
      injectedBytes: median(list.map(entry => entry.total?.injectedBytes)),
      hookMs: median(list.map(entry => entry.craft.hookMs)),
    };
    for (const measure of MEASURES) row[measure] = median(list.map(entry => basis(entry)?.[measure]));
    return row;
  });
  for (const row of rows) {
    const baseline = rows.find(other => other.task === row.task && other.arm === BASELINE);
    if (!baseline || baseline === row) continue;
    row.versus = {
      arm: BASELINE,
      deciding: row.n >= MIN_DECIDING_ATTEMPTS && baseline.n >= MIN_DECIDING_ATTEMPTS,
      ratio: Object.fromEntries(['toolCalls', 'inputTokens', 'seconds', 'injectedBytes'].map(measure => [measure,
        Number.isFinite(row[measure]) && Number.isFinite(baseline[measure]) && baseline[measure] > 0
          ? Math.round((row[measure] / baseline[measure]) * 100) / 100 : null])),
    };
  }
  return rows;
}

const cell = value => value === null || value === undefined ? '—' : String(value);
const ratio = value => value === null || value === undefined ? '—' : `${value}×`;

export function markdown(entries, rows, title = 'Eval results') {
  const out = [`### ${title}`, ''];
  out.push('| Task | Arm | N | Correct | Answered | Basis | Tool calls | Shell commands | Searches | File reads | Package reads | Input tokens | Seconds | Injected bytes | Hook ms | vs vanilla (calls / input / seconds) |');
  out.push('|---|---|---:|---:|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|');
  for (const row of rows) {
    const versus = row.versus
      ? `${ratio(row.versus.ratio.toolCalls)} / ${ratio(row.versus.ratio.inputTokens)} / ${ratio(row.versus.ratio.seconds)}${row.versus.deciding ? '' : ` (N<${MIN_DECIDING_ATTEMPTS}, not deciding)`}`
      : '—';
    out.push(`| ${[row.task, row.arm, row.n, `${row.correct}/${row.graded}`, row.answered === null ? '—' : `${row.answered}/${row.n}`, row.basis,
      ...['toolCalls', 'commands', 'searches', 'fileReads', 'packageReads', 'inputTokens', 'seconds', 'injectedBytes', 'hookMs'].map(m => cell(row[m])),
      versus].join(' | ')} |`);
  }
  out.push('', 'Medians across attempts. Basis "first answer" counts exploration until the agent first wrote the task\'s answer file; "whole trial" counts the whole run. Correctness is the hidden verifier\'s. Input tokens include cached tokens. Unknown values show as —, never 0.', '');
  out.push('<details><summary>Trials</summary>', '');
  out.push('| Trial | Arm | Model | Reward | Craft events | Lookups | Plan | Expected skill | Tool calls | Input tokens | Output tokens | Cost USD | Agent seconds | Error |');
  out.push('|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|');
  for (const entry of entries) {
    out.push(`| ${[entry.trial, entry.arm, entry.model, entry.reward, entry.craft.events, entry.craft.lookups, entry.craft.plan, entry.craft.expectedSkill,
      entry.total?.toolCalls, entry.tokens.input, entry.tokens.output, entry.costUsd, entry.agentSeconds, entry.error ?? ''].map(cell).join(' | ')} |`);
  }
  out.push('', 'Each trial\'s verifier/reward.json holds the task-specific values; a cost the host does not report stays unknown.', '', '</details>', '');
  return out.join('\n');
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  try {
    const args = process.argv.slice(2);
    const option = name => { const at = args.indexOf(name); return at >= 0 ? args[at + 1] : undefined; };
    const positional = args.filter((arg, i) => !arg.startsWith('--') && !['--title', '--root'].includes(args[i - 1]));
    if (positional.length !== 1) throw new Error('usage: npm run eval:summary -- <harbor-job-directory> [--root <repository>] [--title <text>] [--json]');
    const entries = trials(path.resolve(positional[0]), { root: path.resolve(option('--root') ?? '.') });
    const rows = aggregate(entries);
    process.stdout.write(args.includes('--json') ? `${JSON.stringify({ rows, trials: entries }, null, 2)}\n` : markdown(entries, rows, option('--title')));
  } catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 2; }
}
