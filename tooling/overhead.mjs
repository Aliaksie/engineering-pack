#!/usr/bin/env node
// Deterministic overhead each export adds to a host session, measured from the built `dist/`:
//   always   bytes the host shows on every model call (skill listing, instruction files)
//   hooks    bytes each context-injecting hook returns for fixed prompts and edits
//   guidance bytes of the files a routed intent selects
// No model runs; hook latency stays in the benchmark suite. `tests/overhead-budget.test.mjs`
// compares the numbers with checked-in ceilings.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { HOSTS } from '../hosts/registry.mjs';
import { frontmatter } from './lib/capabilities.mjs';

const repository = fileURLToPath(new URL('../', import.meta.url));
export const EXPORTS = Object.keys(HOSTS);
const PACKAGES = ['craft-delivery', 'craft-engineering', 'craft-review', 'craft-stories'];

function skillFiles(exportRoot, host) {
  const { skills } = HOSTS[host];
  if (skills) {
    const folder = path.join(exportRoot, skills);
    return fs.existsSync(folder) ? fs.readdirSync(folder).sort().map(dir => ({ pkg: null, dir, file: path.join(folder, dir, 'SKILL.md') })) : [];
  }
  const base = path.join(exportRoot, host === 'claude' ? 'plugins' : 'packages');
  return PACKAGES.filter(pkg => fs.existsSync(path.join(base, pkg, 'skills'))).flatMap(pkg =>
    fs.readdirSync(path.join(base, pkg, 'skills')).sort().map(dir => ({ pkg, dir, file: path.join(base, pkg, 'skills', dir, 'SKILL.md') })));
}

// One listing entry as the host renders it: `- <name>: <description>`, prefixed `<plugin>:` where
// the skill comes from an installed plugin, with the SKILL.md location where the host shows one.
export function listingEntry(host, { pkg, dir, name, description }) {
  const { listing } = HOSTS[host];
  const shown = pkg ? `${pkg}:${name}` : name;
  const location = listing.paths ? ` (file: r0/${dir}/SKILL.md)` : '';
  return `- ${shown}: ${description}${location}\n`;
}

export function always(exportRoot, host) {
  const skills = skillFiles(exportRoot, host).filter(entry => fs.existsSync(entry.file)).map(entry => {
    const { name, description } = frontmatter(fs.readFileSync(entry.file, 'utf8'));
    return { ...entry, name, description };
  });
  const listing = skills.reduce((sum, entry) => sum + Buffer.byteLength(listingEntry(host, entry)), 0);
  const instructions = Object.fromEntries(HOSTS[host].listing.instructions.map(file => {
    const target = path.join(exportRoot, file);
    return [file, fs.existsSync(target) ? fs.statSync(target).size : 0];
  }));
  const instructionBytes = Object.values(instructions).reduce((a, b) => a + b, 0);
  return { skills: skills.length, listingBytes: listing, instructions, bytes: listing + instructionBytes, evidence: HOSTS[host].listing.evidence };
}

// Prompts and edits with no Craft advantage first, then routed work. Each host runs the same
// sequence in one session; a repeated identical hint should cost nothing.
export const SCENARIOS = [
  { id: 'session-start', lifecycle: 'session-start' },
  { id: 'chat', lifecycle: 'before-prompt', prompt: 'Thanks, that looks good.' },
  { id: 'general-question', lifecycle: 'before-prompt', prompt: 'What is the difference between a process and a thread?' },
  { id: 'literal-search', lifecycle: 'before-prompt', prompt: 'Find where the string "invoice-total" appears.' },
  { id: 'one-line-fix', lifecycle: 'before-prompt', prompt: 'Fix the off-by-one in src/total.mjs: the loop should start at 0.' },
  { id: 'edit-source', lifecycle: 'before-tool', tool: 'edit', file: 'src/total.mjs' },
  { id: 'edit-test', lifecycle: 'before-tool', tool: 'edit', file: 'src/total.test.mjs' },
  { id: 'after-edit', lifecycle: 'after-tool', tool: 'edit', file: 'src/total.mjs' },
  { id: 'callers-question', lifecycle: 'before-prompt', prompt: 'Who calls computeTotal?' },
];
const TOOL = { edit: { claude: 'Edit', codex: 'apply_patch', cursor: 'Write', copilot: 'edit', gemini: 'replace' }, other: {} };

function payload(host, cwd, session, scenario) {
  const file = scenario.file ? path.join(cwd, scenario.file) : undefined;
  if (host === 'copilot') return { sessionId: session, cwd, prompt: scenario.prompt, toolName: TOOL.edit.copilot, toolArgs: file ? { path: file } : {} };
  const tool = scenario.tool ? TOOL[scenario.tool][host] ?? 'Edit' : undefined;
  return { session_id: session, cwd, prompt: scenario.prompt, tool_name: tool, tool_input: file ? { file_path: file } : {}, hook_event_name: HOSTS[host].events[scenario.lifecycle]?.name };
}

function project(root) {
  const git = (...args) => spawnSync('git', ['-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test', '-c', 'commit.gpgsign=false', ...args], { cwd: root });
  fs.mkdirSync(path.join(root, 'src'), { recursive: true });
  fs.writeFileSync(path.join(root, 'src/total.mjs'), 'export function computeTotal(items) {\n  let sum = 0;\n  for (let i = 1; i < items.length; i++) sum += items[i];\n  return sum;\n}\n');
  fs.writeFileSync(path.join(root, 'src/total.test.mjs'), "import { computeTotal } from './total.mjs';\nconsole.assert(computeTotal([1, 2]) === 3);\n");
  fs.writeFileSync(path.join(root, 'src/report.mjs'), "import { computeTotal } from './total.mjs';\nexport const report = items => `invoice-total ${computeTotal(items)}`;\n");
  git('init', '-q'); git('add', '.'); git('commit', '-qm', 'fixture');
}

// Machine-specific absolute paths (project root, runtime location) count as fixed placeholders,
// so the numbers are the same on every machine and runner.
export function portable(text, paths) {
  for (const [value, placeholder] of Object.entries(paths)) {
    for (const form of [value, JSON.stringify(value).slice(1, -1), JSON.stringify(JSON.stringify(value).slice(1, -1)).slice(1, -1)]) text = text.split(form).join(placeholder);
  }
  return text;
}

// Bytes on stdout of every context-injecting hook the export registers, per scenario.
export function hooks(exportRoot, host, binary) {
  const events = HOSTS[host].events ?? {};
  const registered = PACKAGES.flatMap(pkg => {
    const manifest = path.join(repository, 'packages', pkg, 'capability.json');
    return JSON.parse(fs.readFileSync(manifest, 'utf8')).hooks.map(hook => ({ pkg, ...hook }));
  }).filter(hook => events[hook.event]?.context && hook.handler !== 'review-read-only');
  if (!registered.length) return { bytes: 0, scenarios: {} };
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'craft-overhead-')));
  try {
    project(root);
    const session = `overhead-${host}`;
    const scenarios = {};
    for (const scenario of SCENARIOS) {
      let bytes = 0;
      for (const hook of registered.filter(hook => hook.event === scenario.lifecycle)) {
        const args = ['hook', host, hook.event, '--package', hook.pkg, '--handler', hook.handler, '--scope', 'any'];
        const child = spawnSync(binary, args, { cwd: root, input: JSON.stringify(payload(host, root, session, scenario)), encoding: 'utf8', env: { ...process.env, CRAFT_EVIDENCE_WARM: '0' } });
        if (child.status !== 0 && child.status !== 2) throw new Error(`${host} ${hook.pkg} ${hook.event}: ${child.stderr}`);
        bytes += Buffer.byteLength(portable(child.stdout ?? '', { [root]: '<root>', [binary]: '<craft>' }));
      }
      if (events[scenario.lifecycle]?.context) scenarios[scenario.id] = bytes;
    }
    return { bytes: Object.values(scenarios).reduce((a, b) => a + b, 0), scenarios };
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
}

export const INTENTS = [
  { intent: 'implement', files: ['src/total.mjs'] },
  { intent: 'verify', files: ['src/total.test.mjs'] },
  { intent: 'review' }, { intent: 'plan' }, { intent: 'locate' }, { intent: 'investigate' }, { intent: 'explore' },
];

// Bytes of the skill files and required package instructions each routed intent selects.
export function guidance(exportRoot, binary) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-overhead-guidance-'));
  try {
    return Object.fromEntries(INTENTS.map(({ intent, files = [] }) => {
      const child = spawnSync(binary, ['context-report', '--root', root, '--intent', intent, ...files.flatMap(file => ['--file', file]), '--bundle', exportRoot, '--json'], { encoding: 'utf8' });
      if (child.status !== 0) throw new Error(`context-report ${intent}: ${child.stderr}`);
      return [intent, JSON.parse(child.stdout).totals.bytes];
    }));
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
}

export function measure(distRoot, binary, hosts = EXPORTS) {
  return Object.fromEntries(hosts.map(host => {
    const exportRoot = path.join(distRoot, host);
    return [host, { always: always(exportRoot, host), hooks: hooks(exportRoot, host, binary), guidance: guidance(exportRoot, binary) }];
  }));
}

function table(report) {
  const rows = [['host', 'skills', 'listing B', 'instructions B', 'always B', 'hooks B (sequence)', ...INTENTS.map(({ intent }) => intent)]];
  for (const [host, { always: a, hooks: h, guidance: g }] of Object.entries(report)) {
    rows.push([`${host}${a.evidence === 'assumed' ? '*' : ''}`, a.skills, a.listingBytes, a.bytes - a.listingBytes, a.bytes, h.bytes, ...INTENTS.map(({ intent }) => g[intent])]);
  }
  const widths = rows[0].map((_, i) => Math.max(...rows.map(row => String(row[i]).length)));
  return rows.map(row => row.map((cell, i) => String(cell).padStart(widths[i])).join('  ')).join('\n')
    + '\n* listing format assumed from host documentation; not observed in a recorded session\n';
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  const args = process.argv.slice(2);
  const json = args.includes('--json');
  const hosts = args.filter(arg => !arg.startsWith('--'));
  const { nativeBinary } = await import('./lib/native-binary.mjs');
  const report = measure(path.join(repository, 'dist'), nativeBinary(), hosts.length ? hosts : EXPORTS);
  process.stdout.write(json ? `${JSON.stringify(report, null, 2)}\n` : table(report));
}
