import { nativeCommand } from './craft.mjs';
export function groundedContext(root, input, { preferencesFile } = {}) {
  const outcome = nativeCommand(['grounded-context', '--root', root, ...(preferencesFile ? ['--preferences', preferencesFile] : [])], { structured: true, data: input });
  if (outcome.exitCode) throw new Error(outcome.stderr.trimEnd());
  return JSON.parse(outcome.stdout);
}
