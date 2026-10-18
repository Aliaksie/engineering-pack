import fs from 'node:fs';
import path from 'node:path';
import { emitBundle } from '../agents/index.mjs';
import { frontmatter } from '../../lib/capabilities.mjs';
import { rewriteLinks, write } from '../export.mjs';

export function emit(item, packages, output, adapterRoot) {
  const result = emitBundle(item, packages, output, adapterRoot);
  return { ...result, adapter: 'jetbrains' };
}
export function finish(packages, output) {
  const item = packages.find(({ manifest }) => manifest.name === 'craft-engineering');
  if (!item) return;
  const source = path.join(output, 'packages', item.manifest.name, 'skills', 'craft-engineering-foundations', 'SKILL.md');
  const target = path.join(output, '.aiassistant/rules/engineering-pack.md');
  const body = rewriteLinks(frontmatter(fs.readFileSync(source, 'utf8')).body, source, target, resolved => resolved);
  write(output, '.aiassistant/rules/engineering-pack.md', `<!-- Generated from craft-engineering:foundations; edit packages/ and rebuild. -->\n${body}`);
}
