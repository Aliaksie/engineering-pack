// Record what one released craft binary writes in a short Claude session, for the version
// skew replay: `node tests/fixtures/skew/capture.mjs <released-craft-binary> <new-directory>`.
// Runs with an isolated HOME and Git configuration; a fixed author and date make the commit
// the replay recreates identical.
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const [binary, target] = process.argv.slice(2);
if (!binary || !target) throw new Error('usage: capture.mjs <craft-binary> <new-directory>');
if (fs.existsSync(target)) throw new Error(`${target} exists`);
fs.mkdirSync(target, { recursive: true });
const out = fs.realpathSync(target);
const project = path.join(out, 'project');
const home = path.join(out, 'home');
fs.mkdirSync(path.join(home, '.config/craft'), { recursive: true });
fs.mkdirSync(path.join(project, '.craft'), { recursive: true });
fs.mkdirSync(path.join(project, 'src'));

const { CRAFT_ORGANIZATION_CONFIG, CRAFT_PACKAGE_ROOT, ...inherited } = process.env;
const env = { ...inherited, HOME: home, USERPROFILE: home, GIT_CONFIG_NOSYSTEM: '1',
  GIT_AUTHOR_DATE: '2026-11-01T00:00:00Z', GIT_COMMITTER_DATE: '2026-11-01T00:00:00Z' };
const run = (command, args, input) => spawnSync(command, args, { cwd: project, env, input, encoding: 'utf8' });
const git = (...args) => {
  const result = run('git', ['-c', 'core.hooksPath=/dev/null', '-c', 'commit.gpgsign=false',
    '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test', ...args]);
  if (result.status !== 0) throw new Error(result.stderr);
};

fs.writeFileSync(path.join(home, '.config/craft/preferences.json'),
  JSON.stringify({ formatVersion: 1, routing: { sessionSummary: true, debtMarkers: true }, safety: { enabled: true } }));
fs.writeFileSync(path.join(project, '.craft/project.json'), JSON.stringify({
  formatVersion: 1,
  routing: { debtMarkers: true },
  modules: [{ name: 'core', path: 'src', languages: ['rust'], dependsOn: [],
    checks: [{ id: 'core-ok', command: 'true', args: [], stage: 'commit' }] }],
  standards: [{ id: 'commits', anchor: 'conventional-commits' },
    { id: 'changelog-line', summary: 'Observable change adds a CHANGELOG line.', checklist: ['CHANGELOG has a line'], scope: { paths: ['src'] } }],
  safety: { enabled: true },
}));
fs.writeFileSync(path.join(project, 'src/lib.rs'), 'pub fn a() {}\n');
git('init', '-q', '-b', 'main');
git('add', '-A');
git('commit', '-qm', 'chore: start');

const session = 'skew-session-1';
const base = { session_id: session, cwd: project };
const hook = (lifecycle, extra, packages = ['craft-delivery', 'craft-engineering']) => {
  for (const name of packages) {
    const handler = name === 'craft-delivery' ? 'deliver' : 'observe';
    run(binary, ['hook', 'claude', lifecycle, '--package', name, '--handler', handler, '--scope', 'any'],
      JSON.stringify({ ...base, ...extra }));
  }
};
const file = path.join(project, 'src/lib.rs');
hook('session-start', { source: 'startup' });
hook('before-prompt', { prompt: 'implement a second function in src/lib.rs' });
hook('before-tool', { tool_name: 'Edit', tool_input: { file_path: file } });
fs.writeFileSync(file, 'pub fn a() {}\n// TODO: finish b\npub fn b() {}\n');
hook('after-tool', { tool_name: 'Edit', tool_input: { file_path: file }, tool_response: {} });
hook('before-tool', { tool_name: 'Bash', tool_input: { command: 'cargo test' } });
hook('after-tool', { tool_name: 'Bash', tool_input: { command: 'cargo test' },
  tool_response: { stdout: 'running 2 tests\ntest result: ok. 2 passed; 0 failed; 0 ignored\n', stderr: '' } });
run(binary, ['checkpoint-save', '--root', project, '--session', session, '--summary', 'second function']);
run(binary, ['check', '--root', project, '--stage', 'commit', '--session', session]);
hook('before-compact', { trigger: 'auto' });
hook('after-compact', { source: 'compact' });
hook('turn-end', { stop_hook_active: false });
hook('agent-start', { agent_type: 'general' }, ['craft-delivery']);
console.log(`captured into ${out}: copy home/.config/craft/preferences.json, project/.craft/project.json and project/.craft/state (without *.lock)`);
