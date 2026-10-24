#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const repository = fileURLToPath(new URL('../', import.meta.url));
const marker = 'native-bundle.json';
const digest = file => createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const writeJson = (file, value) => fs.writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`);

function checkedDirectory(directory) {
  if (fs.existsSync(directory)) {
    const stat = fs.lstatSync(directory);
    if (!stat.isDirectory() || stat.isSymbolicLink()) throw new Error(`unsafe output directory: ${directory}`);
  } else fs.mkdirSync(directory);
}

function notices(metadata, stage) {
  const packages = new Map(metadata.packages.map(pkg => [pkg.id, pkg]));
  const nodes = new Map(metadata.resolve.nodes.map(node => [node.id, node]));
  const root = metadata.packages.find(pkg => pkg.name === 'craft-application');
  if (!root) throw new Error('native application package missing');
  const selected = new Set();
  const visit = id => {
    if (selected.has(id)) return;
    selected.add(id);
    for (const dependency of nodes.get(id)?.deps ?? []) visit(dependency.pkg);
  };
  visit(root.id);
  const records = [];
  for (const id of [...selected].sort()) {
    const pkg = packages.get(id);
    if (!pkg || !pkg.license) throw new Error(`dependency lacks license metadata: ${id}`);
    if (metadata.workspace_members.includes(id)) continue;
    const directory = path.dirname(pkg.manifest_path);
    let files = fs.readdirSync(directory).filter(name => /^(LICENSE|COPYING|NOTICE)([._-].*)?$/i.test(name))
      .map(name => path.join(directory, name)).filter(file => fs.statSync(file).isFile());
    if (pkg.license_file) files.push(path.resolve(directory, pkg.license_file));
    if (pkg.name === 'tree-sitter-java' && pkg.version === '0.23.5') {
      files.push(path.join(repository, 'tooling/native/notices/tree-sitter-java-0.23.5-LICENSE'));
    }
    files = [...new Set(files)].sort();
    if (!files.length) throw new Error(`dependency has no packaged notice: ${pkg.name}@${pkg.version}`);
    const destination = path.join(stage, 'notices', `${pkg.name}-${pkg.version}`);
    fs.mkdirSync(destination, { recursive: true });
    const retained = [];
    for (const file of files) {
      const name = path.basename(file);
      fs.copyFileSync(file, path.join(destination, name));
      retained.push(`notices/${pkg.name}-${pkg.version}/${name}`);
    }
    // The binding's MIT notice does not describe the bundled SQLite C source.
    if (pkg.name === 'libsqlite3-sys') {
      const source = fs.readFileSync(path.join(directory, 'sqlite3/sqlite3.c'), 'utf8');
      const end = source.indexOf('*/');
      const disclaimer = source.indexOf('The author disclaims copyright');
      const noticeStart = source.lastIndexOf('/*', disclaimer);
      const noticeEnd = source.indexOf('*/', disclaimer);
      if (!source.startsWith('/*') || end < 0 || end > 8192 || disclaimer < 0 || noticeStart < 0 || noticeEnd < 0 || noticeEnd > 8192) throw new Error('SQLite source notice missing');
      fs.writeFileSync(path.join(destination, 'SQLite-NOTICE'), `${source.slice(0, end + 2)}\n${source.slice(noticeStart, noticeEnd + 2)}\n`);
      retained.push(`notices/${pkg.name}-${pkg.version}/SQLite-NOTICE`);
    }
    records.push({ name: pkg.name, version: pkg.version, license: pkg.license, repository: pkg.repository, notices: retained });
  }
  writeJson(path.join(stage, 'dependencies.json'), { scope: 'current-target resolved graph including build dependencies', packages: records });
}

function manifestFiles(directory, relative = '') {
  return fs.readdirSync(path.join(directory, relative), { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name)).flatMap(entry => {
    const name = relative ? `${relative}/${entry.name}` : entry.name;
    if (entry.isSymbolicLink()) throw new Error('bundle contains a symlink');
    if (entry.isDirectory()) return manifestFiles(directory, name);
    return [{ path: name, bytes: fs.statSync(path.join(directory, name)).size, sha256: digest(path.join(directory, name)) }];
  });
}

export function buildNativeBundle() {
  const cargo = process.env.CARGO ?? 'cargo';
  const rustc = process.env.RUSTC ?? 'rustc';
  const run = (command, args) => execFileSync(command, args, { cwd: repository, encoding: 'utf8', maxBuffer: 16 * 1024 * 1024, timeout: 300000 });
  const compiler = run(rustc, ['-vV']);
  const target = compiler.match(/^host: ([a-zA-Z0-9_-]+)$/m)?.[1];
  if (!target) throw new Error('cannot identify current native target');
  const dist = path.join(repository, 'dist');
  checkedDirectory(dist);
  const parent = path.join(dist, 'project-evidence');
  checkedDirectory(parent);
  const output = path.join(parent, target);
  if (fs.existsSync(output)) {
    checkedDirectory(output);
    const existing = path.join(output, marker);
    if (!fs.existsSync(existing) || fs.lstatSync(existing).isSymbolicLink()
      || JSON.parse(fs.readFileSync(existing, 'utf8')).target !== target) throw new Error('refusing to replace unmanaged native bundle');
  }
  run(cargo, ['build', '--release', '--locked', '--offline', '--package', 'craft-application', '--target', target]);
  const metadata = JSON.parse(run(cargo, ['metadata', '--format-version', '1', '--locked', '--offline', '--filter-platform', target]));
  const executable = `craft-project-evidence${process.platform === 'win32' ? '.exe' : ''}`;
  const binary = path.join(metadata.target_directory, target, 'release', executable);
  const versionResponse = JSON.parse(execFileSync(binary, [], { input: JSON.stringify({ formatVersion: 1, operation: 'version' }), encoding: 'utf8', timeout: 10000 }));
  if (versionResponse.ok !== true || versionResponse.formatVersion !== 1 || versionResponse.result?.schemaVersion !== 1) throw new Error('incompatible native binary');
  const stage = fs.mkdtempSync(path.join(parent, '.bundle-'));
  try {
    for (const relative of ['cli/project-evidence.mjs', 'application/project-evidence.mjs', 'platform/node/project-evidence.mjs', 'LICENSE']) {
      fs.mkdirSync(path.dirname(path.join(stage, relative)), { recursive: true });
      fs.copyFileSync(path.join(repository, relative), path.join(stage, relative));
    }
    // Preserve the application's public platform port without shipping unrelated runtime modules.
    fs.writeFileSync(path.join(stage, 'platform/index.mjs'), "export { projectEvidenceProcess } from './node/project-evidence.mjs';\n");
    fs.mkdirSync(path.join(stage, 'native-bin'));
    fs.copyFileSync(binary, path.join(stage, 'native-bin', executable));
    fs.chmodSync(path.join(stage, 'native-bin', executable), 0o755);
    notices(metadata, stage);
    fs.writeFileSync(path.join(stage, 'README.md'), `# Project evidence local bundle\n\nTarget: ${target}. Requires Node 22+ and Git on PATH; Rust is not required.\n\nRun from this directory:\n\n\`\`\`sh\nnode cli/project-evidence.mjs version --enable\nnode cli/project-evidence.mjs refresh --enable --root /absolute/repository\nnode cli/project-evidence.mjs query --enable --root /absolute/repository --value symbol --verify\n\`\`\`\n\nMove the whole directory together. Omit --enable to disable native access. No hooks or host configuration are installed. Keep the previous bundle directory for rollback; source evidence remains in each selected repository's private .craft state. Replacing a bundle does not migrate that state. Other targets are unverified. See dependencies.json and notices/ for dependency notices.\n`);
    const manifest = { formatVersion: 1, target, profile: 'release', protocolVersion: 1, ...versionResponse.result,
      compiler: compiler.split('\n')[0], sourceRevision: run('git', ['rev-parse', 'HEAD']).trim(),
      sourceDirty: run('git', ['status', '--porcelain', '--untracked-files=normal']).trim().length > 0,
      cargoLockSha256: digest(path.join(repository, 'Cargo.lock')), files: manifestFiles(stage) };
    writeJson(path.join(stage, marker), manifest);
    // Only generated directories carrying our target marker may be replaced.
    if (fs.existsSync(output)) fs.rmSync(output, { recursive: true });
    fs.renameSync(stage, output);
    return { output, target, binaryVersion: manifest.binaryVersion, files: manifest.files.length, bytes: manifest.files.reduce((sum, file) => sum + file.bytes, 0) };
  } finally {
    if (fs.existsSync(stage)) fs.rmSync(stage, { recursive: true });
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    if (process.argv.length !== 2) throw new Error('native-bundle takes no arguments; it builds the current target into dist/project-evidence');
    console.log(JSON.stringify(buildNativeBundle(), null, 2));
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
