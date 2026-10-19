#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { validatePlaybookCatalog } from '../application/routing/catalog.mjs';
import { synchronizeVersion } from './version.mjs';
import { files, loadCapabilities } from './lib/capabilities.mjs';
import { setupControls } from '../hosts/onboarding.mjs';
import { copy, rewriteLinks, runtimeImports, skillName, write, writeJson } from './export/export.mjs';
import * as claude from './export/claude/index.mjs';
import * as codex from './export/codex/index.mjs';
import * as agents from './export/agents/index.mjs';
import * as cursor from './export/cursor/index.mjs';
import * as copilot from './export/copilot/index.mjs';
import * as gemini from './export/gemini/index.mjs';
import * as geminiExtension from './export/gemini/extension.mjs';
import * as jetbrains from './export/jetbrains/index.mjs';

export const ADAPTERS = { claude, codex, cursor, copilot, gemini, agents, 'gemini-extension': geminiExtension, jetbrains };
const MARKER = '.capability-build.json';

export function build(repo, target) {
  const version = synchronizeVersion(repo, undefined, true);
  if (!Object.hasOwn(ADAPTERS, target)) throw new Error(`unknown adapter ${target}`);
  const packages = loadCapabilities(repo);
  validatePlaybookCatalog(JSON.parse(fs.readFileSync(path.join(repo, 'engine/routing/catalog.json'), 'utf8')));
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
    const reports = packages.map((item) => ADAPTERS[target].emit(item, packages, stage, path.join(repo, 'hosts')));
    ADAPTERS[target].finish(packages, stage);
    const packageFolder = target === 'claude' ? 'plugins' : 'packages';
    for (const { manifest } of packages) {
      const destination = path.join(stage, packageFolder, manifest.name);
      // Copy the canonical portable setup policy into every standalone package.
      write(destination, 'SETUP.md', fs.readFileSync(new URL('../packages/craft-engineering/standards/session-setup.md', import.meta.url), 'utf8'));
      fs.appendFileSync(path.join(destination, 'AGENTS.md'), '\nOn first use in a conversation, read [setup and session visibility](SETUP.md) and give the brief setup notice described there.\n');
      const adapter = reports.find(entry => entry.package === manifest.name);
      writeJson(destination, 'SETUP.json', {
        formatVersion: 1, adapter: target, hooks: adapter.hooks, sessionAdvice: adapter.sessionAdvice,
        controls: setupControls(target)
      });
      for (const folder of ['engine', 'application', 'platform', 'integrations', 'cli']) copy(path.join(repo, folder), path.join(destination, 'support', folder));
      // Canonical package entry points import shared runtime source. Installed packages
      // resolve that same implementation locally, without fallback loaders or siblings.
      for (const file of files(destination).filter(file => file.endsWith('.mjs') && !path.relative(destination,file).startsWith(`support${path.sep}`))) {
        const source = fs.readFileSync(file,'utf8');
        const relocated = runtimeImports(source,file,destination);
        if (source !== relocated) fs.writeFileSync(file,relocated);
      }
      write(destination, 'platform-runtime/session.mjs', fs.readFileSync(path.join(repo, 'hosts/session.mjs'), 'utf8'));
      write(destination, 'support/session.mjs', `#!/usr/bin/env node\nimport { sessionStatus } from './application/session/status.mjs';\nimport { flushTelemetry } from './application/telemetry/observe.mjs';\nimport { readProject, readInput } from './application/project/configuration.mjs';\nimport { observeNativeSession, hookDiagnostic } from './application/context-delivery/prepare.mjs';\nimport { runSession } from '../platform-runtime/session.mjs';\nawait runSession(sessionStatus, readInput, readProject, ${JSON.stringify(target)}, { run:observeNativeSession, diagnostic:hookDiagnostic });\nawait flushTelemetry();\n`);
    }
    const license = path.join(repo, 'LICENSE');
    if (fs.existsSync(license)) {
      fs.copyFileSync(license, path.join(stage, 'LICENSE'));
      for (const { manifest } of packages) fs.copyFileSync(license, path.join(stage, packageFolder, manifest.name, 'LICENSE'));
    }
    for (const folder of ['engine', 'application', 'platform', 'integrations', 'cli']) copy(path.join(repo, folder), path.join(stage, folder));
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
    writeJson(stage, 'adapter-report.json', { formatVersion: 1, version, adapter: target, packages: reports });
    writeJson(stage, MARKER, { formatVersion: 1, version, adapter: target });
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
    process.stderr.write('usage: node tooling/build.mjs [all|claude|codex|cursor|copilot|gemini|agents|gemini-extension|jetbrains]\n');
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
