import { emitBundle } from '../agents/index.mjs';
import { sessionAdvice, write, writeJson } from '../export.mjs';

// Guidance-only extension: lifecycle enforcement remains an explicit overlay choice.
export function emit(item, packages, output, adapterRoot) {
  const result = emitBundle(item, packages, output, adapterRoot, 'agents', 'skills');
  return { ...result, adapter: 'gemini-extension', sessionAdvice: sessionAdvice('gemini-extension', item.manifest.name) };
}
export function finish(packages, output) {
  writeJson(output, 'gemini-extension.json', {
    name: 'engineering-pack', version: packages[0].manifest.version,
    description: 'Portable engineering practices and workflows.',
    contextFileName: 'GEMINI.md'
  });
  write(output, 'GEMINI.md', '<!-- Generated; edit packages/ and rebuild. -->\nRead [AGENTS.md](AGENTS.md) for the optional capability index. Follow the consumer repository instructions.\n');
}
