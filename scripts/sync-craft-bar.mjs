import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const engineering = path.join(repository, 'packages/craft-engineering');
const targets = ['backend-engineering', 'foundations', 'react-engineering', 'security-engineering', 'testing-engineering'];
export const BEGIN = '<!-- craft-bar:begin — generated from packages/craft-engineering/standards/craft-bar.md. Edit the source, not this block. -->';
export const END = '<!-- craft-bar:end -->';

export function skillDirs() {
  const directory = path.join(engineering, 'skills');
  if (!fs.existsSync(directory)) return [];
  return fs.readdirSync(directory, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && targets.includes(entry.name))
    .map((entry) => entry.name).sort();
}

export function render(skill) {
  const source = fs.readFileSync(path.join(engineering, 'standards/craft-bar.md'), 'utf8');
  const content = source.replace(/\r\n/g, '\n').replace(/^<!--[^]*?-->\n/, '');
  const filled = content.replaceAll('{{SKILL}}', skill).replace(/\n*$/, '');
  return [BEGIN, filled, END, ''].join('\n');
}

function blockRange(body) {
  const begins = [...body.matchAll(/<!-- craft-bar:begin\b[^\r\n]*?-->/g)];
  const ends = [...body.matchAll(/<!-- craft-bar:end\s*-->/g)];
  const tokens = [...body.matchAll(/<!-- craft-bar:(?:begin|end)\b/g)];
  if (!tokens.length) return null;
  if (tokens.length !== begins.length + ends.length) throw new Error('craft-bar contains an incomplete marker');
  if (begins.length !== 1 || ends.length !== 1) throw new Error('craft-bar requires exactly one begin marker and one end marker');
  const start = begins[0].index;
  const end = ends[0].index + ends[0][0].length;
  if (ends[0].index <= start) throw new Error('craft-bar end marker precedes begin marker');
  const before = body.slice(0, start);
  const after = body.slice(end);
  const afterBegin = body.slice(start + begins[0][0].length);
  const beforeEnd = body.slice(0, ends[0].index);
  if ((before && !before.endsWith('\n')) || !/^\r?\n/.test(afterBegin)
    || !beforeEnd.endsWith('\n') || (after && !/^\r?\n/.test(after))) {
    throw new Error('craft-bar markers must occupy separate lines');
  }
  return { start, end };
}

export function apply(body, skill) {
  const range = blockRange(body);
  if (!range) return null;
  const newline = body.includes('\r\n') ? '\r\n' : '\n';
  const generated = render(skill).slice(0, -1).replaceAll('\n', newline);
  return body.slice(0, range.start) + generated + body.slice(range.end);
}

function inspect() {
  const skills = skillDirs();
  const errors = [];
  const updates = [];
  if (!skills.length) errors.push('no craft-engineering skills were found; craft-bar check is reading nothing');
  for (const skill of skills) {
    const file = path.join(engineering, 'skills', skill, 'SKILL.md');
    const label = path.relative(repository, file).split(path.sep).join('/');
    try {
      const body = fs.readFileSync(file, 'utf8');
      const next = apply(body, skill);
      if (next === null) errors.push(`${label} has no craft-bar markers`);
      else if (next !== body) updates.push({ file, label, next });
    } catch (error) {
      errors.push(`${label}: ${error.message}`);
    }
  }
  return { errors, updates };
}

export function check() {
  const { errors, updates } = inspect();
  return [...errors, ...updates.map(({ label }) => `${label} is out of date with standards/craft-bar.md; run npm run sync:craft-bar`)];
}

export function write() {
  const { errors, updates } = inspect();
  if (errors.length) throw new Error(errors.join('\n'));
  for (const { file, next } of updates) fs.writeFileSync(file, next);
  return updates.map(({ label }) => label);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    if (process.argv.includes('--check')) {
      const errors = check();
      if (errors.length) throw new Error(errors.join('\n'));
      process.stdout.write(`${skillDirs().length} craft skills match standards/craft-bar.md\n`);
    } else {
      const files = write();
      process.stdout.write(files.length ? `Updated ${files.join(', ')}\n` : 'Already up to date.\n');
    }
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  }
}
