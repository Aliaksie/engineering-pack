import path from 'node:path';
import { emitBundle } from '../agents/index.mjs';
import { HOSTS, hookMapping } from '../../../hosts/registry.mjs';
import { write, writeJson } from '../export.mjs';

export function workspace(platform) {
  return {
    emit(item, packages, output, adapterRoot) { return emitBundle(item, packages, output, adapterRoot, platform, HOSTS[platform].skills); },
    finish(packages, output) {
      const events = {};
      for (const { manifest } of packages) for (const definition of manifest.hooks) {
        const mapping = hookMapping(platform, definition);
        if (!mapping) continue;
        // Workspace overlays preserve relative paths. These fragments are launched from
        // the consumer's project root and must be merged, not replace existing settings.
        const command = `node "./packages/${manifest.name}/platform-hooks/${definition.name}.mjs"`;
        if (platform === 'cursor') (events[mapping.name] ??= []).push({ command });
        else if (platform === 'copilot') (events[mapping.name] ??= []).push({ type: 'command', bash: command, powershell: command, cwd: '.', timeoutSec: 10 });
        else (events[mapping.name] ??= []).push({ hooks: [{ name: `craft-${manifest.name}-${definition.name}`, type: 'command', command, timeout: 10000 }] });
      }
      writeJson(output, HOSTS[platform].config, platform === 'gemini' ? { hooks: events } : { version: 1, hooks: events });
      if (platform === 'gemini') write(output, 'GEMINI.md', 'Read [AGENTS.md](AGENTS.md) for the shared capability index.\n');
      if (platform === 'copilot') write(output, '.github/copilot-instructions.md', 'Read [AGENTS.md](../AGENTS.md) for the shared capability index.\n');
    }
  };
}
