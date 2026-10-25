import path from 'node:path';
import { emitBundle } from '../agents/index.mjs';
import { HOSTS, hookMapping } from '../../../hosts/registry.mjs';
import { write, writeJson } from '../export.mjs';
import { hookArguments } from '../launcher.mjs';

export function workspace(platform) {
  return {
    emit(item, packages, output, adapterRoot) { return emitBundle(item, packages, output, adapterRoot, platform, HOSTS[platform].skills); },
    finish(packages, output) {
      const events = {};
      for (const item of packages) for (const definition of item.manifest.hooks) {
        const { manifest } = item;
        const mapping = hookMapping(platform, definition);
        if (!mapping) continue;
        // Workspace overlays preserve relative paths. These fragments are launched from
        // the consumer's project root and must be merged, not replace existing settings.
        const args = hookArguments(platform, definition, item).join(' ');
        const command = `"./packages/${manifest.name}/bin/craft" ${args}`;
        if (platform === 'cursor') (events[mapping.name] ??= []).push({ command });
        // Gemini runs hooks with bash, or PowerShell on Windows, which does not run a command
        // that starts with a quoted path; kebab-case package paths need no quoting.
        else if (platform === 'gemini') (events[mapping.name] ??= []).push({ hooks: [{ name: `craft-${manifest.name}-${definition.name}`, type: 'command', command: `./packages/${manifest.name}/bin/craft ${args}`, timeout: 10000 }] });
        else if (platform === 'copilot') (events[mapping.name] ??= []).push({ type: 'command', bash: command, powershell: `& "./packages/${manifest.name}/bin/craft.cmd" ${args}`, cwd: '.', timeoutSec: 10 });
        else (events[mapping.name] ??= []).push({ hooks: [{ name: `craft-${manifest.name}-${definition.name}`, type: 'command', command, timeout: 10000 }] });
      }
      writeJson(output, HOSTS[platform].config, platform === 'gemini' ? { hooks: events } : { version: 1, hooks: events });
      if (platform === 'gemini') write(output, 'GEMINI.md', 'Read [AGENTS.md](AGENTS.md) for the shared capability index.\n');
      if (platform === 'copilot') write(output, '.github/copilot-instructions.md', 'Read [AGENTS.md](../AGENTS.md) for the shared capability index.\n');
    }
  };
}
