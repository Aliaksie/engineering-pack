import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { nativeCommand } from './craft.mjs';

// `craft grounded-context` over one input object, written to a file as a caller keeps it.
export function groundedContext(root, input, { preferencesFile } = {}) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-grounding-'));
  try {
    const data = path.join(directory, 'input.json');
    fs.writeFileSync(data, JSON.stringify(input));
    const outcome = nativeCommand(['grounded-context', '--root', root, '--data', data, ...(preferencesFile ? ['--preferences', preferencesFile] : [])], { structured: true });
    if (outcome.exitCode) throw new Error(outcome.stderr.trimEnd());
    return JSON.parse(outcome.stdout);
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
}
