#!/usr/bin/env node
import { withTelemetry, flushTelemetry } from '../../../application/telemetry/observe.mjs';
import fs from 'node:fs';
import path from 'node:path';
import { snapshot } from '../../../application/review.mjs';
try {
  const options = {};
  const allowed = ['root', 'base', 'head', 'provider', 'repository', 'change-id', 'output'];
  const args = process.argv.slice(2);
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index].slice(2);
    if (!args[index].startsWith('--') || !allowed.includes(key) || !args[index + 1] || Object.hasOwn(options, key)) throw new Error('usage: prepare-review.mjs --base ref --head ref [--root repo] [--output file]');
    options[key] = args[index + 1];
  }
  if (!options.output) throw new Error('--output is required; keep the diff in an artifact rather than the model transcript');
  const result = withTelemetry(path.resolve(options.root ?? '.'), {}, () => snapshot({ root: path.resolve(options.root ?? '.'), base: options.base, head: options.head, provider: options.provider, repository: options.repository, changeId: options['change-id'] ?? null }));
  const text = `${JSON.stringify(result, null, 2)}\n`;
  fs.writeFileSync(path.resolve(options.output), text, { flag: 'wx', mode: 0o600 });
  process.stdout.write(`${JSON.stringify({ artifact: path.resolve(options.output), baseSha: result.baseSha, headSha: result.headSha, files: result.files.length })}\n`);
} catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 2; }
finally { await flushTelemetry(); }
