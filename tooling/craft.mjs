#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { nativeBinary } from './lib/native-binary.mjs';

// `npm run <command> -- ...` in a source checkout: the development `craft`, with this
// checkout as the package root whose guidance session commands account.
const repository = fileURLToPath(new URL('../', import.meta.url));
try {
  const binary = nativeBinary();
  const child = spawnSync(binary, process.argv.slice(2), { stdio: 'inherit', shell: false,
    env: { ...process.env, CRAFT_PACKAGE_ROOT: process.env.CRAFT_PACKAGE_ROOT || repository } });
  if (child.error) throw child.error;
  process.exitCode = child.status ?? 1;
} catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 1; }
