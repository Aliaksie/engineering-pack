import fs from 'node:fs';
import path from 'node:path';
export function projectData(root) {
  const file = path.join(root, '.capabilities', 'project.json');
  for (const candidate of [path.dirname(file), file]) {
    const stat = fs.lstatSync(candidate, { throwIfNoEntry: false });
    if (stat?.isSymbolicLink()) throw new Error('project configuration may not be a symlink');
  }
  if (!fs.existsSync(file)) return { formatVersion: 1 };
  return JSON.parse(fs.readFileSync(file, 'utf8'));
}
