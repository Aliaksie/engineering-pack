#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { build } from './build.mjs';
import { copy, rewriteLinks, write, writeJson } from './export/export.mjs';
import { files } from './lib/capabilities.mjs';
import { validateMarketplace } from './validate-marketplace.mjs';
import { packVersion } from './version.mjs';

const targets = ['gemini-extension', 'claude', 'codex', 'jetbrains'];
const marker = '.engineering-distribution.json';

// One branch exposes native entry points while preserving each adapter's resource layout.
export function assembleDistribution(repo) {
  const version = packVersion(repo);
  const dist = path.join(repo, 'dist');
  const output = path.join(dist, 'distribution');
  if (fs.lstatSync(dist).isSymbolicLink()) throw new Error('dist must not be a symlink');
  for (const target of targets) {
    const directory = path.join(dist, target);
    files(directory); // Reject symlinks before copying any generated content.
    const metadata = JSON.parse(fs.readFileSync(path.join(directory, '.capability-build.json'), 'utf8'));
    if (metadata.adapter !== target) throw new Error(`invalid ${target} build`);
    if (metadata.version !== version) throw new Error(`stale ${target} version`);
  }
  if (fs.existsSync(output)) {
    if (fs.lstatSync(output).isSymbolicLink()) throw new Error('distribution must not be a symlink');
    const metadata = path.join(output, marker);
    if (!fs.existsSync(metadata) || fs.lstatSync(metadata).isSymbolicLink()
      || JSON.parse(fs.readFileSync(metadata, 'utf8')).formatVersion !== 1) throw new Error('refusing to replace unmanaged distribution');
  }
  const stage = fs.mkdtempSync(path.join(dist, '.distribution-'));
  try {
    copy(path.join(dist, 'gemini-extension'), stage);
    for (const target of ['claude', 'codex', 'jetbrains']) copy(path.join(dist, target), path.join(stage, target));
    const read = name => JSON.parse(fs.readFileSync(path.join(stage, name), 'utf8'));
    const claude = read('claude/.claude-plugin/marketplace.json');
    for (const plugin of claude.plugins) plugin.source = `./claude/${plugin.source.slice(2)}`;
    writeJson(stage, '.claude-plugin/marketplace.json', claude);
    const codex = read('codex/.agents/plugins/marketplace.json');
    for (const plugin of codex.plugins) plugin.source.path = `./codex/${plugin.source.path.slice(2)}`;
    writeJson(stage, '.agents/plugins/marketplace.json', codex);
    // Hosts cache individual plugins. Links outside a plugin must remain reachable
    // without assuming that the rest of the marketplace was installed alongside it.
    for (const directory of [...claude.plugins.map(plugin => plugin.source), ...codex.plugins.map(plugin => plugin.source.path)]) {
      const pluginRoot = path.join(stage, directory);
      for (const file of files(pluginRoot).filter(file => file.endsWith('.md'))) {
        const text = rewriteLinks(fs.readFileSync(file, 'utf8'), file, file, resolved => {
          const relative = path.relative(pluginRoot, resolved);
          if (relative !== '..' && !relative.startsWith(`..${path.sep}`)) return resolved;
          return `https://github.com/Aliaksie/engineering-pack/blob/distribution-next/${path.relative(stage, resolved).split(path.sep).map(encodeURIComponent).join('/')}`;
        });
        fs.writeFileSync(file, text);
      }
    }

    writeJson(stage, marker, { formatVersion: 1, version, generatedFrom: 'packages/', targets });
    write(stage, 'README.md', '# Craft Capabilities distribution\n\nGenerated from the portable source; do not edit this branch.\n\nSee [installation and updates](docs/INSTALL.md).\nFor JetBrains native chat, use the complete [JetBrains bundle](jetbrains/AGENTS.md).\n');
    const errors = validateMarketplace(stage);
    if (errors.length) throw new Error(errors.join('\n'));
    fs.rmSync(output, { recursive: true, force: true });
    fs.renameSync(stage, output);
    return output;
  } catch (error) {
    fs.rmSync(stage, { recursive: true, force: true });
    throw error;
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const repo = fileURLToPath(new URL('../', import.meta.url));
  for (const target of targets) build(repo, target);
  process.stdout.write(`Prepared ${assembleDistribution(repo)}\n`);
}
