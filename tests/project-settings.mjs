// Effective settings as `craft settings` resolves them for a project root.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { run } from './craft.mjs';

export const readProject = (root, { preferencesFile } = {}) =>
  run(['settings', '--root', root, ...(preferencesFile ? ['--preferences', preferencesFile] : [])]).settings;

// The settings of a scratch project whose `.craft/project.json` is `project`.
export function validateProject(project) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-project-'));
  try {
    fs.mkdirSync(path.join(root, '.craft'));
    fs.writeFileSync(path.join(root, '.craft/project.json'), JSON.stringify(project));
    return readProject(root);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
}
