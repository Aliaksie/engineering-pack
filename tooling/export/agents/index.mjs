import fs from 'node:fs';
import path from 'node:path';
import { copy, emitHooks, preparePackage, report, rewriteLinks, writeJson } from '../export.mjs';

export function emitBundle(item, packages, output, adapterRoot, platform = 'agents', skillRoot = '.agents/skills') {
  const manifest = item.manifest;
  const destination = path.join(output, 'packages', manifest.name);
  const skills = preparePackage(item, packages, destination, platform);
  for (const name of skills) {
    const target = path.join(output, skillRoot, name);
    copy(path.join(destination, 'skills', name), target);
    const resourceRoot = path.relative(target, destination).split(path.sep).join('/');
    const sourceSkill = path.join(destination, 'skills', name);
    // Rebase links in all skill resources, including cross-skill links and package context.
    const visit = (directory) => {
      for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
        const file = path.join(directory, entry.name);
        if (entry.isDirectory()) { visit(file); continue; }
        if (!file.endsWith('.md')) continue;
        const sourceFile = path.join(sourceSkill, path.relative(target, file));
        const text = rewriteLinks(fs.readFileSync(file, 'utf8'), sourceFile, file, (resolved) => {
          const relative = path.relative(path.join(destination, 'skills'), resolved);
          return relative.startsWith('..') ? resolved : path.join(output, skillRoot, relative);
        });
        fs.writeFileSync(file, text);
      }
    };
    visit(target);
    const file = path.join(target, 'SKILL.md');
    const text = fs.readFileSync(file, 'utf8').replace('`../..` is the package root', `\`${resourceRoot}\` is the package root`);
    fs.writeFileSync(file, `${text}\nPackage tools and shared assets: \`${resourceRoot}\` relative to this skill directory.\n`);
  }
  if (manifest.mcp.length) writeJson(destination, 'mcp-declarations.json', { servers: manifest.mcp });
  if (platform !== 'agents') emitHooks(item, destination, platform, adapterRoot);
  return report(manifest, platform, skills, manifest.hooks.length ? platform === 'agents' ? 'unsupported-not-installed' : 'native-hook-overlay' : 'none', manifest.mcp.length ? 'declarations-only-not-registered' : 'none');
}
export function finish() {}

export function emit(item, packages, output, adapterRoot) { return emitBundle(item, packages, output, adapterRoot); }
