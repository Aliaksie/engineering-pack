import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

// Deterministic module-map checks. Rules and their rationale: docs/adr/0002-module-map-and-boundaries.md.
const KINDS = ['rust-crate', 'host-adapter', 'content', 'build-tooling', 'schema', 'docs', 'tests', 'examples', 'eval-suite', 'ci-config'];
const BANNED_RUST_MODULES = ['utils', 'common', 'helpers', 'misc', 'shared', 'core'];
const IMPURE_ENGINE = /\bstd::(?:fs|process|net|env)\b|\buse\s+std::\{[^}]*\b(?:fs|process|net|env)\b|\b(?:SystemTime|Instant)::now\b/;

// `**/` spans directories, a trailing `**` matches the rest, `*` stays within one segment.
export function glob(pattern) {
  let source = '';
  for (let index = 0; index < pattern.length; index++) {
    if (pattern.startsWith('**/', index)) { source += '(?:.*/)?'; index += 2; }
    else if (pattern.startsWith('**', index)) { source += '.*'; index += 1; }
    else if (pattern[index] === '*') source += '[^/]*';
    else source += pattern[index].replace(/[.+?^${}()|[\]\\]/g, '\\$&');
  }
  return new RegExp(`^${source}$`);
}
const matches = (patterns, file) => patterns.some(pattern => glob(pattern).test(file));
const lines = text => (text.match(/\n/g) ?? []).length;
const list = value => value === '*' ? '*' : [...value].sort().join(', ') || '—';

function trackedFiles(root) {
  const result = spawnSync('git', ['ls-files', '-z', '--cached', '--others', '--exclude-standard'], { cwd: root, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  if (result.status !== 0) throw new Error(`git ls-files failed: ${result.stderr.trim()}`);
  return [...new Set(result.stdout.split('\0').filter(Boolean))].filter(file => fs.existsSync(path.join(root, file))).sort();
}

function imports(root, file) {
  const source = fs.readFileSync(path.join(root, file), 'utf8');
  return [...source.matchAll(/(?:from\s*|import\s*|import\(\s*)(['"])([^'"]+)\1/g)].map(match => match[2])
    .filter(specifier => specifier.startsWith('.')).map(specifier => path.posix.normalize(path.posix.join(path.posix.dirname(file), specifier)));
}

// Minimal Cargo manifest reading: dependency names, path targets and lint inheritance.
function cargo(text) {
  const dependencies = [];
  let section = '';
  for (const raw of text.split('\n')) {
    const line = raw.replace(/#.*/, '').trim();
    const header = line.match(/^\[([^\]]+)\]$/);
    if (header) { section = header[1]; continue; }
    if (!/(^|\.)(dependencies|dev-dependencies|build-dependencies)$/.test(section)) continue;
    const entry = line.match(/^([A-Za-z0-9_-]+)(?:\.workspace)?\s*=\s*(.*)$/);
    if (entry) dependencies.push({ name: entry[1], path: entry[2].match(/path\s*=\s*"([^"]+)"/)?.[1] });
  }
  return { name: text.match(/^\s*name\s*=\s*"([^"]+)"/m)?.[1], dependencies, lints: /\[lints\]\s*\nworkspace\s*=\s*true/.test(text) };
}

function adrModules(text) {
  const section = text.split(/^## /m).find(part => part.startsWith('Module map'));
  if (!section) return null;
  const rows = new Map();
  for (const line of section.split('\n')) {
    const cells = line.split('|').slice(1, -1).map(cell => cell.trim());
    const name = cells[0]?.match(/^`([^`]+)\/`$/)?.[1];
    if (name && cells.length >= 4) rows.set(name, { kind: cells[1], dependsOn: cells[2], shipped: cells[3] });
  }
  return rows;
}

export function checkArchitecture(root) {
  const errors = [];
  const fail = (rule, subject, message) => errors.push({ rule, subject, message });
  const manifestFile = path.join(root, 'architecture.json');
  if (!fs.existsSync(manifestFile)) return { errors: [{ rule: 'A1', subject: 'architecture.json', message: 'module manifest is missing' }] };
  const manifest = JSON.parse(fs.readFileSync(manifestFile, 'utf8'));
  const modules = manifest.modules ?? {};
  const files = trackedFiles(root);
  const owner = file => file.includes('/') ? file.split('/')[0] : null;

  // A1 every top-level path is declared; A2 files use the module's allowed shapes.
  for (const file of files) {
    const name = owner(file);
    if (!name) { if (!manifest.rootFiles?.includes(file)) fail('A1', file, 'undeclared root file; add it to rootFiles in architecture.json'); continue; }
    const module = modules[name];
    if (!module) { fail('A1', `${name}/`, 'undeclared top-level module; follow "Adding a top-level module" in ADR 0002 and declare it in architecture.json'); continue; }
    const relative = file.slice(name.length + 1);
    if (!matches(module.allow ?? [], relative)) fail('A2', file, `file shape not allowed in ${module.kind} module ${name}/`);
  }
  for (const [name, module] of Object.entries(modules)) {
    if (!KINDS.includes(module.kind)) fail('A3', `${name}/`, `unknown kind ${module.kind}; new kinds require amending ADR 0002`);
    if (!/^[^.;]+\.$/.test(module.responsibility ?? '')) fail('A3', `${name}/`, 'responsibility must be one sentence');
    if (module.dependsOn !== '*') for (const dependency of module.dependsOn ?? []) if (!modules[dependency]) fail('A3', `${name}/`, `depends on undeclared module ${dependency}`);
  }

  // A3 the accepted ADR records the same map; changing the map means changing the decision.
  const decision = manifest.decision && path.join(root, manifest.decision);
  if (!decision || !fs.existsSync(decision)) fail('A3', manifest.decision ?? 'decision', 'module map decision record is missing');
  else {
    const text = fs.readFileSync(decision, 'utf8');
    if (!/^Status: Accepted\b/m.test(text)) fail('A3', manifest.decision, 'decision record is not accepted');
    const rows = adrModules(text) ?? new Map();
    for (const [name, module] of Object.entries(modules)) {
      const row = rows.get(name);
      const expected = { kind: module.kind, dependsOn: list(module.dependsOn ?? []), shipped: module.shipped ? 'yes' : 'no' };
      if (!row) fail('A3', `${name}/`, `missing from the ADR 0002 module map table`);
      else for (const key of Object.keys(expected)) if (row[key] !== expected[key]) fail('A3', `${name}/`, `ADR 0002 ${key} "${row[key]}" differs from architecture.json "${expected[key]}"`);
    }
    for (const name of rows.keys()) if (!modules[name]) fail('A3', `${name}/`, 'ADR 0002 lists a module that architecture.json does not declare');
  }

  // A4 Rust workspace and crate dependency graph.
  const crates = Object.entries(modules).filter(([, module]) => module.kind === 'rust-crate');
  const workspaceFile = path.join(root, 'Cargo.toml');
  const workspace = fs.existsSync(workspaceFile) ? fs.readFileSync(workspaceFile, 'utf8') : '';
  const members = [...(workspace.match(/members\s*=\s*\[([^\]]*)\]/)?.[1] ?? '').matchAll(/"([^"]+)"/g)].map(match => match[1]).sort();
  if (list(members) !== list(crates.map(([name]) => name))) fail('A4', 'Cargo.toml', `workspace members [${list(members)}] must equal rust-crate modules [${list(crates.map(([name]) => name))}]`);
  if (crates.length && !/unsafe_code\s*=\s*"forbid"/.test(workspace)) fail('A4', 'Cargo.toml', 'workspace must forbid unsafe code');
  for (const [name, module] of crates) {
    const file = path.join(root, name, 'Cargo.toml');
    if (!fs.existsSync(file)) { fail('A4', `${name}/Cargo.toml`, 'crate manifest is missing'); continue; }
    const crate = cargo(fs.readFileSync(file, 'utf8'));
    if (crate.name !== module.crate) fail('A4', `${name}/Cargo.toml`, `crate name ${crate.name} must be ${module.crate}`);
    if (!crate.lints) fail('A4', `${name}/Cargo.toml`, 'crate must inherit workspace lints');
    for (const dependency of crate.dependencies) {
      if (dependency.path) {
        const target = path.posix.normalize(path.posix.join(name, dependency.path));
        if (!module.dependsOn.includes(target)) fail('A4', `${name}/Cargo.toml`, `path dependency on ${target}/ is not in dependsOn`);
      } else if (!module.externalDependencies?.includes(dependency.name)) fail('A4', `${name}/Cargo.toml`, `external dependency ${dependency.name} is not declared in externalDependencies`);
    }
  }

  // A5 engine purity; A9 Rust module naming.
  for (const file of files.filter(file => file.endsWith('.rs') && crates.some(([name]) => file.startsWith(`${name}/src/`)))) {
    const text = fs.readFileSync(path.join(root, file), 'utf8');
    const segments = file.replace(/\.rs$/, '').split('/');
    if (segments.some(segment => BANNED_RUST_MODULES.includes(segment)) || [...text.matchAll(/\bmod\s+([a-z_]+)/g)].some(match => BANNED_RUST_MODULES.includes(match[1]))) fail('A9', file, `generic module names (${BANNED_RUST_MODULES.join(', ')}) are not allowed; name the domain`);
    if (!text.trim()) fail('A9', file, 'empty Rust module; create modules only for existing behavior');
    const production = text.split(/#\[cfg\(test\)\]/)[0];
    if (modules[file.split('/')[0]].crate === 'craft-engine' && IMPURE_ENGINE.test(production)) fail('A5', file, 'engine must not use filesystem, process, network, environment or ambient clock');
  }

  // A6 cross-module JS imports follow dependsOn and target declared entry points.
  const javascript = files.filter(file => file.endsWith('.mjs'));
  const entryPoints = new Map();
  for (const [name, module] of Object.entries(modules)) for (const entry of module.entryPoints ?? []) entryPoints.set(entry, name);
  const graph = new Map(javascript.map(file => [file, imports(root, file)]));
  const importedAcross = new Set();
  for (const [file, targets] of graph) {
    const from = owner(file);
    const module = modules[from];
    if (!module) continue;
    for (const target of targets) {
      const to = owner(target);
      if (!to || to === from || !modules[to]) continue;
      if (module.dependsOn === '*') continue;
      importedAcross.add(target);
      if (!module.dependsOn.includes(to)) fail('A6', file, `imports ${target}, but ${from}/ does not depend on ${to}/`);
      else if (entryPoints.get(target) !== to) fail('A6', file, `imports ${target}, which is not a declared entry point of ${to}/`);
    }
  }
  for (const [entry, name] of entryPoints) if (!importedAcross.has(entry)) fail('A6', entry, `stale entry point of ${name}/; no other module imports it`);

  // A7 runtime JS must be reachable from a build-time consumer; tests do not keep code alive.
  const runtime = file => { const module = modules[owner(file)]; return module && module.dependsOn !== '*' && !file.endsWith('.test.mjs'); };
  const reached = new Set();
  const buildTime = file => ['build-tooling', 'ci-config'].includes(modules[owner(file)]?.kind);
  const stack = javascript.filter(buildTime);
  while (stack.length) {
    const file = stack.pop();
    if (reached.has(file) || !graph.has(file)) continue;
    reached.add(file);
    stack.push(...graph.get(file));
  }
  for (const file of javascript.filter(file => runtime(file) && !reached.has(file))) fail('A7', file, 'runtime JavaScript is unreachable from any build-time consumer; delete it');

  // A8 JavaScript only in host adapters and build/test modules, adapters import nothing; A10 thin host adapters.
  for (const file of javascript.filter(runtime)) {
    const module = modules[owner(file)];
    if (module.kind !== 'host-adapter') { fail('A8', file, `JavaScript in ${module.kind} module ${owner(file)}/; runtime behavior belongs in craft (ADR 0001)`); continue; }
    const count = lines(fs.readFileSync(path.join(root, file), 'utf8'));
    if (module.maxFileLines && count > module.maxFileLines) fail('A10', file, `${count} lines exceeds the ${module.maxFileLines}-line adapter limit; move behavior into the Rust application`);
  }
  for (const [name, module] of Object.entries(modules)) if (module.kind === 'host-adapter' && module.dependsOn.length) fail('A8', `${name}/`, 'host adapters invoke craft and import no other module');

  // A11 exports contain only shipped modules.
  const dist = path.join(root, 'dist');
  if (fs.existsSync(dist)) for (const bundle of fs.readdirSync(dist, { withFileTypes: true }).filter(entry => entry.isDirectory())) {
    // Dot folders in exports are host configuration (for example Copilot's .github), not repository modules.
    for (const entry of fs.readdirSync(path.join(dist, bundle.name))) if (!entry.startsWith('.') && modules[entry] && !modules[entry].shipped) fail('A11', `dist/${bundle.name}/${entry}`, `${entry}/ is not shipped but appears in an export`);
  }
  return { errors };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const root = fileURLToPath(new URL('../', import.meta.url));
  const { errors } = checkArchitecture(root);
  if (process.argv.includes('--json')) process.stdout.write(`${JSON.stringify({ errors }, null, 2)}\n`);
  else {
    for (const error of errors) process.stderr.write(`${error.rule} ${error.subject}: ${error.message}\n`);
    if (!errors.length) process.stdout.write('architecture: ok\n');
  }
  if (errors.length) process.exitCode = 1;
}
