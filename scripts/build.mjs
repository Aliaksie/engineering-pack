#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { validatePlaybookCatalog } from '../runtime/validate-catalog.mjs';
import { files, loadCapabilities } from './lib/capabilities.mjs';
import { copy, rewriteLinks, skillName, write, writeJson } from '../adapters/shared/export.mjs';
import * as claude from '../adapters/claude/index.mjs';
import * as codex from '../adapters/codex/index.mjs';
import * as agents from '../adapters/agents/index.mjs';
import * as cursor from '../adapters/cursor/index.mjs';
import * as copilot from '../adapters/copilot/index.mjs';
import * as gemini from '../adapters/gemini/index.mjs';
import * as geminiExtension from '../adapters/gemini/extension.mjs';
import * as jetbrains from '../adapters/jetbrains/index.mjs';

export const ADAPTERS = { claude, codex, cursor, copilot, gemini, agents, 'gemini-extension': geminiExtension, jetbrains };
const MARKER = '.capability-build.json';

export function build(repo, target) {
  if (!Object.hasOwn(ADAPTERS, target)) throw new Error(`unknown adapter ${target}`);
  const packages = loadCapabilities(repo);
  validatePlaybookCatalog(JSON.parse(fs.readFileSync(path.join(repo, 'runtime/catalog.json'), 'utf8')));
  const dist = path.join(repo, 'dist');
  if (fs.existsSync(dist) && fs.lstatSync(dist).isSymbolicLink()) throw new Error('dist must not be a symlink');
  fs.mkdirSync(dist, { recursive: true });
  const output = path.join(dist, target);
  if (fs.existsSync(output)) {
    if (fs.lstatSync(output).isSymbolicLink()) throw new Error('adapter output must not be a symlink');
    const marker = path.join(output, MARKER);
    if (fs.readdirSync(output).length && (!fs.existsSync(marker) || fs.lstatSync(marker).isSymbolicLink()
      || JSON.parse(fs.readFileSync(marker, 'utf8')).adapter !== target)) throw new Error(`refusing to replace unmanaged output: ${output}`);
  }
  const stage = fs.mkdtempSync(path.join(dist, `.${target}-`));
  try {
    const reports = packages.map((item) => ADAPTERS[target].emit(item, packages, stage, path.join(repo, 'adapters')));
    ADAPTERS[target].finish(packages, stage);
    const packageFolder = target === 'claude' ? 'plugins' : 'packages';
    for (const { manifest } of packages) {
      const destination = path.join(stage, packageFolder, manifest.name);
      copy(path.join(repo, 'runtime'), path.join(destination, 'support/runtime'));
      write(destination, 'platform-runtime/session.mjs', fs.readFileSync(path.join(repo, 'adapters/shared/session.mjs'), 'utf8'));
      write(destination, 'support/session.mjs', `#!/usr/bin/env node\nimport { sessionAdvice, readSessionInput } from './runtime/session.mjs';\nimport { readProject } from './runtime/project.mjs';\nimport { runSession } from '../platform-runtime/session.mjs';\nawait runSession(sessionAdvice, readSessionInput, readProject, ${JSON.stringify(target)});\n`);
    }
    const license = path.join(repo, 'LICENSE');
    if (fs.existsSync(license)) {
      fs.copyFileSync(license, path.join(stage, 'LICENSE'));
      for (const { manifest } of packages) fs.copyFileSync(license, path.join(stage, packageFolder, manifest.name, 'LICENSE'));
    }
    copy(path.join(repo, 'runtime'), path.join(stage, 'runtime'));
    copy(path.join(repo, 'docs'), path.join(stage, 'docs'));
    // Shared documentation can link into any package. Rebase those links to the
    // adapter's package folder and native skill names, just as package docs do.
    for (const source of files(path.join(repo, 'docs')).filter(file => file.endsWith('.md'))) {
      const targetFile = path.join(stage, path.relative(repo, source));
      const text = rewriteLinks(fs.readFileSync(source, 'utf8'), source, targetFile, resolved => {
        for (const item of packages) {
          const relative = path.relative(item.root, resolved);
          if (relative === '..' || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) continue;
          const parts = relative.split(path.sep);
          if (parts[0] === 'skills' && parts.length > 1) parts[1] = skillName(item.manifest.name, parts[1], target);
          if (relative === 'capability.json') parts[0] = 'source-capability.json';
          return path.join(stage, packageFolder, item.manifest.name, ...parts);
        }
        return path.join(stage, path.relative(repo, resolved));
      });
      fs.writeFileSync(targetFile, text);
    }
    if (fs.existsSync(path.join(repo, 'schemas'))) copy(path.join(repo, 'schemas'), path.join(stage, 'schemas'));
    write(stage, 'AGENTS.md', `# AI Capability Packages\n\nRead the target repository's own instructions before using these optional capabilities.\n\n${reports.map((entry) => `- ${entry.package}: ${target === 'claude' ? 'plugins' : 'packages'}/${entry.package}/AGENTS.md`).join('\n')}\n\nSee adapter-report.json for feature support and limitations.\n`);
    writeJson(stage, 'adapter-report.json', { formatVersion: 1, adapter: target, packages: reports });
    writeJson(stage, MARKER, { formatVersion: 1, adapter: target });
    fs.rmSync(output, { recursive: true, force: true });
    fs.renameSync(stage, output);
    return reports;
  } catch (error) {
    fs.rmSync(stage, { recursive: true, force: true });
    throw error;
  }
}

// Check generated exports without re-running a model or contacting a service.
export function checkBuild(repo, target) {
  const directory = path.join(repo, 'dist', target);
  const reportFile = path.join(directory, 'adapter-report.json');
  if (!fs.existsSync(reportFile)) return [`missing ${target} export; run npm run build`];
  const report = JSON.parse(fs.readFileSync(reportFile, 'utf8'));
  const packages = loadCapabilities(repo);
  const errors = [];
  for (const { manifest } of packages) {
    const entry = report.packages.find((entry) => entry.package === manifest.name);
    if (!entry || entry.version !== manifest.version) errors.push(`${target}: stale or missing ${manifest.name}`);
    const base = path.join(directory, target === 'claude' ? 'plugins' : 'packages', manifest.name);
    if (!fs.existsSync(path.join(base, 'AGENTS.md'))) errors.push(`${target}: missing package instructions`);
  }
  files(directory); // Reject symlinks in the export too.
  return errors;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  const args = process.argv.slice(2);
  const target = args.length ? args[0] : 'all';
  if (args.length > 1 || (target !== 'all' && !Object.hasOwn(ADAPTERS, target))) {
    process.stderr.write('usage: node scripts/build.mjs [all|claude|codex|cursor|copilot|gemini|agents|gemini-extension|jetbrains]\n');
    process.exitCode = 2;
  } else {
    const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
    try {
      for (const name of target === 'all' ? Object.keys(ADAPTERS) : [target]) {
        const reports = build(repo, name);
        process.stdout.write(`${name}: exported ${reports.length} capability packages to dist/${name}\n`);
      }
    } catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 1; }
  }
}
