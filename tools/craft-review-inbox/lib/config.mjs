import fs from 'node:fs';
import path from 'node:path';

const part = /^[a-zA-Z0-9][a-zA-Z0-9_.-]*$/;
export function readConfig({ cwd = process.cwd(), config } = {}) {
  let file;
  if (config) file = path.resolve(cwd, config);
  else {
    let directory = path.resolve(cwd);
    for (;;) {
      const candidate = path.join(directory, '.pr-loop/config.json');
      if (fs.existsSync(candidate)) { file = candidate; break; }
      const parent = path.dirname(directory);
      if (parent === directory) break;
      directory = parent;
    }
  }
  if (!file) throw new Error('Review inbox needs .pr-loop/config.json or --config <file>; shell config is not executed.');
  file = fs.realpathSync(file);
  const raw = JSON.parse(fs.readFileSync(file, 'utf8'));
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) throw new Error('inbox config must be an object');
  const allowed = ['formatVersion', 'host', 'owner', 'me', 'repos', 'home', 'artifacts', 'scratch', 'staleDays', 'tickHours'];
  for (const key of Object.keys(raw)) if (!allowed.includes(key)) throw new Error(`unknown inbox config field: ${key}`);
  if (raw.formatVersion !== 1 || typeof raw.host !== 'string' || !/^[a-zA-Z0-9.-]+(?::[0-9]+)?$/.test(raw.host)
    || typeof raw.owner !== 'string' || typeof raw.me !== 'string' || !part.test(raw.owner) || !part.test(raw.me) || !raw.repos || typeof raw.repos !== 'object' || Array.isArray(raw.repos) || !Object.keys(raw.repos).length) throw new Error('config requires formatVersion 1, explicit host, owner, me and repo map');
  for (const [short, name] of Object.entries(raw.repos)) if (!part.test(short) || typeof name !== 'string' || !part.test(name) || (['.', '..'].includes(short) || short.endsWith('.') || /^(?:con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(short)) || ['.', '..'].includes(name)) throw new Error('invalid inbox repo map');
  const foldedAliases = Object.keys(raw.repos).map((alias) => alias.toLowerCase());
  if (new Set(foldedAliases).size !== foldedAliases.length) throw new Error('repository aliases must be unique ignoring case');
  for (const key of ['staleDays', 'tickHours']) if (raw[key] !== undefined && (!Number.isFinite(raw[key]) || raw[key] < 0)) throw new Error(`invalid ${key}`);
  const root = path.resolve(path.dirname(file), '..');
  const directory = (key, fallback) => {
    if (raw[key] === undefined) return fallback;
    if (typeof raw[key] !== 'string' || !raw[key].trim()) throw new Error(`invalid ${key}`);
    return path.resolve(root, raw[key]);
  };
  const home = directory('home', path.join(root, '.pr-loop'));
  return { ...raw, file, root, home, artifacts: directory('artifacts', path.join(home, 'artifacts')), scratch: directory('scratch', path.join(home, 'scratch')), state: path.join(home, 'state'), staleDays: raw.staleDays ?? 5, tickHours: raw.tickHours ?? 6 };
}
export function repoName(config, short) {
  if (!Object.hasOwn(config.repos, short)) throw new Error(`unknown repository: ${short}`);
  return config.repos[short];
}
export function prNumber(value) {
  if (!/^[1-9][0-9]*$/.test(String(value)) || !Number.isSafeInteger(Number(value))) throw new Error('PR number must be a positive integer');
  return Number(value);
}
