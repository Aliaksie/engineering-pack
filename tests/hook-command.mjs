import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

// A package hook as its host runs it: the generated launcher with the registered arguments.
export function hookPackage(exportRoot, host, pkg) {
  return path.join(exportRoot, host === 'claude' ? 'plugins' : 'packages', pkg);
}
export function hookCommand(packageRoot, host, hook) {
  const manifest = JSON.parse(fs.readFileSync(path.join(packageRoot, 'source-capability.json'), 'utf8'));
  const definition = manifest.hooks.find(entry => entry.name === hook);
  if (!definition) throw new Error(`unknown hook ${hook}`);
  const launcher = path.join(packageRoot, 'bin', 'craft');
  const args = ['hook', host, definition.event, '--package', manifest.name, '--handler', definition.handler, '--scope', definition.tool === 'shell' ? 'shell' : 'any'];
  return { launcher, args, exists: fs.existsSync(launcher) };
}
export function runHook(packageRoot, host, hook, { input = '', cwd, env = process.env } = {}) {
  const { launcher, args } = hookCommand(packageRoot, host, hook);
  const options = { cwd, env, input, encoding: 'utf8' };
  // Windows runs the cmd launcher; Node starts .cmd files only through a shell.
  return process.platform === 'win32'
    ? spawnSync(`"${launcher}.cmd"`, args, { ...options, shell: true })
    : spawnSync(launcher, args, options);
}
