import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { randomUUID } from 'node:crypto';
export function projectRoot(cwd) {
  let directory = path.resolve(cwd);
  for (let depth = 0; depth < 30; depth++) {
    if (fs.existsSync(path.join(directory, '.craft/project.json'))) return directory;
    if (fs.existsSync(path.join(directory, '.git'))) return directory;
    const parent = path.dirname(directory);
    if (directory === parent) break;
    directory = parent;
  }
  return path.resolve(cwd);
}
export const realRoot = root => fs.realpathSync.native(root);
export const bundleRoot = () => path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..', path.basename(path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..')) === 'support' ? '..' : '.');
export const clock = () => { const ms = Date.now(); return { ms, iso: new Date(ms).toISOString() }; };
export const uniqueId = () => randomUUID();
export const runtimeVersion = () => JSON.parse(fs.readFileSync(new URL('./pack-version.json', import.meta.url), 'utf8')).version;
export const catalogData = () => JSON.parse(fs.readFileSync(new URL('../../engine/routing/catalog.json', import.meta.url), 'utf8'));
export const sourceIdentity = files => files.map(file => fs.readFileSync(new URL(`../../${file}`, import.meta.url), 'utf8')).join('\n');
