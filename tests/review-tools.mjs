// The native review tools as installed packages run them: `craft review-*` over artifact files.
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

export function craft(args, options = {}) {
  const result = spawnSync(process.env.CRAFT_NATIVE_BINARY, args, { encoding: 'utf8', ...options });
  if (result.error) throw result.error;
  return result;
}

// The command's JSON result; a failed command throws its diagnostic.
function json(args, options) {
  const result = craft(args, options);
  if (result.status !== 0) throw new Error(result.stderr.trimEnd());
  return JSON.parse(result.stdout);
}

let written = 0;
function artifact(directory, value) {
  const file = path.join(directory, `review-artifact-${process.pid}-${++written}.json`);
  fs.writeFileSync(file, JSON.stringify(value));
  return file;
}

// The snapshot artifact `review-prepare` writes for a committed comparison.
export function prepareSnapshot(directory, { root, base, head, provider, repository, changeId }) {
  const output = path.join(directory, `review-snapshot-${process.pid}-${++written}.json`);
  const options = { provider, repository, 'change-id': changeId };
  json(['review-prepare', '--root', root, '--base', base, '--head', head, '--output', output,
    ...Object.entries(options).filter(([, value]) => value !== undefined).flatMap(([name, value]) => [`--${name}`, String(value)])]);
  return JSON.parse(fs.readFileSync(output, 'utf8'));
}

export const checkReview = (directory, snapshot, review) =>
  json(['review-check', '--snapshot', artifact(directory, snapshot), '--review', artifact(directory, review)]);

export const planPublication = (directory, snapshot, review, { currentBase, currentHead, authorized }) =>
  json(['review-publication', '--snapshot', artifact(directory, snapshot), '--review', artifact(directory, review),
    '--current-base', currentBase, '--current-head', currentHead, ...(authorized === true ? ['--authorized'] : [])]);
