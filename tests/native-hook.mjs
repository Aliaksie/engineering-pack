import { spawnSync } from 'node:child_process';

// The native `host` decision for host-neutral hook facts, as `craft hook` evaluates it.
export function nativeHook(facts) {
  const child = spawnSync(process.env.CRAFT_NATIVE_BINARY, [], { input: JSON.stringify({ formatVersion: 1, operation: 'host', input: facts }), encoding: 'utf8' });
  const response = JSON.parse(child.stdout);
  if (!response.ok) throw new Error(response.error.message);
  return response.result;
}
