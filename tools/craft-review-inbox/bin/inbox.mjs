import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { readConfig } from '../lib/config.mjs';
import { withLock } from '../lib/io.mjs';
import { render } from '../lib/render.mjs';
import { checkout, doctor, mark, recordPosted, scan, tick } from '../lib/inbox.mjs';

export function execute(argv, { cwd = process.cwd(), exec } = {}) {
  const [action, ...args] = argv;
  const flags = { doctor: [], scan: ['repo'], tick: ['force', 'hours'], render: [], checkout: [], mark: ['head', 'verdict', 'note'], 'record-posted': ['file', 'stdin'], config: [] };
  if (!Object.hasOwn(flags, action)) throw new Error('usage: node inbox.mjs <config|doctor|scan|tick|render|checkout|mark|record-posted> [--config file] [options]');
  const options = {};
  const positional = [];
  for (let index = 0; index < args.length; index++) {
    const arg = args[index];
    if (!arg.startsWith('--')) { positional.push(arg); continue; }
    const name = arg.slice(2);
    if (!['config', ...flags[action]].includes(name) || Object.hasOwn(options, name)) throw new Error(`unknown or duplicate option: ${arg}`);
    if (['force', 'stdin'].includes(name)) options[name] = true;
    else {
      const value = args[++index];
      if (value === undefined || value.startsWith('--')) throw new Error(`missing value: ${arg}`);
      options[name] = value;
    }
  }
  const arity = action === 'mark' ? 3 : ['checkout', 'record-posted'].includes(action) ? 2 : 0;
  if (positional.length !== arity) throw new Error(`${action} expects ${arity} positional arguments`);
  const config = readConfig({ cwd, config: options.config });
  const [short, number, event] = positional;
  if (action === 'config') return { root: config.root, home: config.home, state: config.state, artifacts: config.artifacts, scratch: config.scratch, host: config.host, repos: config.repos };
  if (action === 'doctor') return doctor(config, { exec });
  if (action === 'scan') return scan(config, { repo: options.repo, exec });
  if (action === 'tick') return tick(config, { force: options.force, ...(options.hours !== undefined ? { hours: Number(options.hours) } : {}), exec });
  if (action === 'render') return withLock(config, () => render(config));
  if (action === 'checkout') return checkout(config, short, number, { exec });
  if (action === 'mark') return mark(config, short, number, event, options);
  return recordPosted(config, short, number, { file: options.file && path.resolve(cwd, options.file), ...(options.stdin ? { text: fs.readFileSync(0, 'utf8') } : {}) });
}
if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  try {
    const result = execute(process.argv.slice(2));
    process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
    if (result.ok === false || result.complete === false) process.exitCode = 1;
  } catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 2; }
}
