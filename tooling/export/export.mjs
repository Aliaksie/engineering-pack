import fs from 'node:fs';
import path from 'node:path';
import { files, frontmatter, stringify } from '../lib/capabilities.mjs';
import { hookMapping } from '../../hosts/registry.mjs';
import { sessionCapabilities } from '../../hosts/session.mjs';

export function write(root, file, text) {
  const target = path.join(root, file);
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.writeFileSync(target, text);
}
export const writeJson = (root, file, value) => write(root, file, stringify(value));
export function copy(source, target) {
  fs.mkdirSync(target, { recursive: true });
  for (const file of files(source)) {
    const dest = path.join(target, path.relative(source, file));
    fs.mkdirSync(path.dirname(dest), { recursive: true });
    fs.copyFileSync(file, dest);
    fs.chmodSync(dest, fs.statSync(file).mode & 0o777);
  }
}

// Rebase canonical shared-runtime imports into a self-contained installed package.
export function runtimeImports(source, file, destination) {
  return source.replace(/(['"])(?:\.\.\/){3}(application|engine|platform|integrations|cli)\//g, (_match, quote, folder) => {
    const relative = path.relative(path.dirname(file),path.join(destination,'support',folder)).split(path.sep).join('/');
    return `${quote}${relative.startsWith('.') ? relative : `./${relative}`}/`;
  });
}

export function skillName(pkg, name, platform) {
  return platform === 'claude' ? name : `${pkg}-${name}`;
}

export function renderText(text, packages, platform) {
  for (const { manifest } of packages) {
    const components = [...manifest.skills.map((entry) => path.basename(entry)), ...manifest.workflows.map((entry) => `workflow-${path.basename(entry, '.md')}`)];
    for (const name of components) {
      if (platform !== 'claude') text = text.replaceAll(`${manifest.name}:${name}`, `${manifest.name}-${name}`);
    }
  }
  return text;
}

// Rebase actual Markdown links when native skill names change. Logical prose references
// and filesystem references are different concerns.
export function rewriteLinks(text, sourceFile, targetFile, mapTarget, paths = path) {
  return text.replace(/\]\(([^)\s]+)\)/g, (match, href) => {
    if (/^(?:[a-z]+:|#)/i.test(href)) return match;
    const hash = href.indexOf('#');
    const pathname = hash === -1 ? href : href.slice(0, hash);
    const fragment = hash === -1 ? '' : href.slice(hash);
    if (!pathname) return match;
    const resolved = paths.resolve(paths.dirname(sourceFile), pathname);
    const mapped = mapTarget(resolved);
    // Windows drive letters are filesystem roots, not URL schemes.
    const mappedHref = !paths.isAbsolute(mapped) && /^[a-z]+:/i.test(mapped) ? mapped : paths.relative(paths.dirname(targetFile), mapped).split(paths.sep).join('/');
    return `](${mappedHref}${fragment})`;
  });
}

function packageDocument(text, sourceFile, targetFile, item, packages, destination, platform) {
  return rewriteLinks(renderText(text, packages, platform), sourceFile, targetFile, (resolved) => {
    const relative = path.relative(item.root, resolved);
    // Files outside the package (e.g. shared docs) retain the package's root-relative layout.
    const parts = relative.split(path.sep);
    if (parts[0] === 'skills' && parts.length > 1 && platform !== 'claude') {
      parts[1] = skillName(item.manifest.name, parts[1], platform);
    }
    if (relative === 'capability.json') return path.join(destination, 'source-capability.json');
    return path.resolve(destination, ...parts);
  });
}

export function preparePackage(item, packages, destination, platform) {
  copy(item.root, destination);
  fs.renameSync(path.join(destination, 'capability.json'), path.join(destination, 'source-capability.json'));
  const { manifest } = item;
  // Render qualified references throughout documents, including supporting references.
  for (const file of files(destination).filter((file) => file.endsWith('.md'))) {
    fs.writeFileSync(file, packageDocument(fs.readFileSync(file, 'utf8'), path.join(item.root, path.relative(destination, file)), file, item, packages, destination, platform));
  }
  const skillsDir = path.join(destination, 'skills');
  const entries = [];
  for (const folder of manifest.skills) {
    const info = frontmatter(fs.readFileSync(path.join(item.root, folder, 'SKILL.md'), 'utf8'));
    entries.push({ name: info.name, info, source: path.join(item.root, folder) });
  }
  for (const workflow of manifest.workflows) {
    const info = frontmatter(fs.readFileSync(path.join(item.root, workflow), 'utf8'));
    entries.push({ name: `workflow-${info.name}`, info });
  }
  fs.rmSync(skillsDir, { recursive: true, force: true });
  for (const entry of entries) {
    const name = skillName(manifest.name, entry.name, platform);
    const target = path.join(skillsDir, name);
    if (entry.source) copy(entry.source, target);
    let body = entry.source
      ? packageDocument(entry.info.body, path.join(entry.source, 'SKILL.md'), path.join(target, 'SKILL.md'), item, packages, destination, platform)
      : renderText(entry.info.body, packages, platform);
    // Bare local skill citations are logical IDs in source. Avoid touching file/reference paths.
    if (platform !== 'claude') for (const local of manifest.skills) {
      const id = path.basename(local);
      body = body.replaceAll(`\`${id}\``, `\`${manifest.name}-${id}\``);
    }
    write(target, 'SKILL.md', `---\nname: ${name}\ndescription: ${JSON.stringify(entry.info.description)}\n---\n\n<!-- Generated from packages/; edit the portable source and rebuild. -->\n\nRead the shared [package instructions](../../AGENTS.md) when using this capability.\n\n${body.trim()}\n`);
    for (const file of files(target).filter((file) => file.endsWith('.md') && path.basename(file) !== 'SKILL.md')) {
      fs.writeFileSync(file, packageDocument(fs.readFileSync(file, 'utf8'), path.join(entry.source, path.relative(target, file)), file, item, packages, destination, platform));
    }
  }
  return entries.map((entry) => skillName(manifest.name, entry.name, platform));
}

export function emitHooks(item, destination, platform, adapterRoot) {
  if (!item.manifest.hooks.length) return;
  for (const [source, name] of [['hooks.mjs','hook.mjs'],['registry.mjs','registry.mjs']]) write(destination, `platform-runtime/${name}`, fs.readFileSync(path.join(adapterRoot, source), 'utf8'));
  const rootVariable = platform === 'claude' ? '${CLAUDE_PLUGIN_ROOT}' : '${PLUGIN_ROOT}';
  const events = {};
  for (const definition of item.manifest.hooks) {
    const mapping = hookMapping(platform, definition);
    if (!mapping) continue;
    const wrapper = `platform-hooks/${definition.name}.mjs`;
    write(destination, wrapper, `#!/usr/bin/env node\nimport { evaluate } from '../${definition.handler}';\nimport { runHook } from '../platform-runtime/hook.mjs';\nimport { prepareEvent, reuseRoutingHint, commitRoutingHint, hookFailure, hookDefault, flushTelemetry } from '../support/application/context-delivery/prepare.mjs';\nawait runHook(evaluate, '${platform}', ${JSON.stringify(definition)}, prepareEvent, { reuse: reuseRoutingHint, commit: commitRoutingHint, failure: hookFailure, default: hookDefault, flush: flushTelemetry });\n`);
    fs.chmodSync(path.join(destination, wrapper), 0o755);
    const hooks = [{ type: 'command', command: `node "${rootVariable}/${wrapper}"` }];
    const matcher = mapping.matcher ?? (definition.tool === 'shell' ? '^Bash$' : undefined);
    (events[mapping.name] ??= []).push({ ...(matcher ? { matcher } : {}), hooks });
  }
  if (platform === 'claude' || platform === 'codex') writeJson(destination, 'platform-hooks/hooks.json', { hooks: events });
}

export function mcpServers(manifest, platform) {
  return Object.fromEntries(manifest.mcp.map(({ name, transport, ...config }) => [name,
    transport === 'http' ? { type: platform === 'claude' ? 'http' : 'streamable-http', ...config } : { type: 'stdio', ...config }
  ]));
}

export function report(manifest, platform, skills, hookStatus, mcpStatus) {
  return {
    package: manifest.name, version: manifest.version, adapter: platform, sourceManifest: 'source-capability.json', skills,
    workflows: 'exported-as-skills', instructions: 'AGENTS.md-linked-from-skills',
    standards: 'included-on-demand', evals: 'included-not-executed', tools: 'included-not-executed',
    hooks: hookStatus, mcp: mcpStatus,
    hookMappings: manifest.hooks.map((definition) => ({ id: definition.name, normalizedEvent: definition.event, ...(hookMapping(platform, definition) ?? { supported: false }) })),
    runtime: 'support/cli/main.mjs',
    sessionAdvice: { entry: 'support/session.mjs', ...sessionCapabilities(platform), activation: 'explicit-only; no statusline settings or hooks changed' },
    limitations: platform === 'agents' ? ['Lifecycle hooks are not installed.', 'MCP declarations require host-specific registration.']
      : platform === 'codex' ? ['Local plugin export; hooks require host support and trust. Live host loading has not been tested.']
      : ['Live host loading has not been tested.']
  };
}
