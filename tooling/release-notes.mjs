import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseVersion } from './version.mjs';

export function changelogNotes(text, version) {
  parseVersion(version);
  const sections = [...text.matchAll(/^## ([^\r\n]+)\r?$/gm)];
  const matches = sections.filter(section => section[1] === version);
  if (matches.length !== 1) throw new Error('release requires exactly one changelog entry for its version');
  const section = matches[0], index = sections.indexOf(section);
  const body = text.slice(section.index + section[0].length, sections[index + 1]?.index).trim();
  if (!body || !/\S/.test(body.replace(/^#+[^\r\n]*$/gm, ''))) throw new Error('release changelog entry is empty');
  return `${body}\n`;
}

export const releaseNotes = (root, version) => changelogNotes(fs.readFileSync(path.join(root, 'CHANGELOG.md'), 'utf8'), version);

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const [version, output, ...extra] = process.argv.slice(2);
    if (!version || !output || extra.length) throw new Error('usage: node tooling/release-notes.mjs VERSION OUTPUT');
    const root = fileURLToPath(new URL('../', import.meta.url));
    fs.writeFileSync(output, releaseNotes(root, version), { flag: 'wx' });
  } catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 1; }
}
