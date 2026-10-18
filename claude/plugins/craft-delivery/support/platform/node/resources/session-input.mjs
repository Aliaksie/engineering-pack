import fs from 'node:fs';
export function readSessionInput(file) {
  if (fs.lstatSync(file).isSymbolicLink() || !fs.statSync(file).isFile() || fs.statSync(file).size > 65536) throw new Error('session input must be a bounded regular JSON file');
  return JSON.parse(fs.readFileSync(file, 'utf8'));
}

