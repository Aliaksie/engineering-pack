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
// The package root and arguments of a registered command in any of its forms: the host shell
// selecting the runtime itself (sh or PowerShell), or a plain launcher call (Gemini).
export function registeredHook(text) {
  const selected = /^\$?r ?= ?"([^"]+)"; .*(?:exec "\$r\/bin\/craft"|& "\$r\\bin\\craft\.cmd") (hook \S.*?)(?:; exit \$LASTEXITCODE)?$/.exec(text);
  if (selected) {
    const [, root, args] = selected;
    // The runtime itself gets the same arguments as the launcher it falls back to.
    if (!text.includes(`"$b" ${args}; `) && !text.includes(`& $b ${args}; `)) throw new Error(`runtime and launcher arguments differ: ${text}`);
    return { root, args, direct: true };
  }
  const plain = /^"?([^" ]+?)\/bin\/craft"? (hook \S.*)$/.exec(text);
  if (!plain) throw new Error(`unrecognized hook command: ${text}`);
  return { root: plain[1], args: plain[2], direct: false };
}
export function runHook(packageRoot, host, hook, { input = '', cwd, env = process.env } = {}) {
  const { launcher, args } = hookCommand(packageRoot, host, hook);
  const options = { cwd, env, input, encoding: 'utf8' };
  // Windows runs the cmd launcher; Node starts .cmd files only through a shell.
  return process.platform === 'win32'
    ? spawnSync(`"${launcher}.cmd"`, args, { ...options, shell: true })
    : spawnSync(launcher, args, options);
}
