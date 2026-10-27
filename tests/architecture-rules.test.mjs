import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import assert from 'node:assert/strict';
import test from 'node:test';
import { checkArchitecture, glob } from '../tooling/architecture.mjs';

const adrRow = (name, kind, deps, shipped) => `| \`${name}/\` | ${kind} | ${deps} | ${shipped} | Fixture |`;
function baseline() {
  return {
    'architecture.json': {
      formatVersion: 1, decision: 'docs/adr/0002.md', rootFiles: ['.gitignore', 'Cargo.toml', 'architecture.json'],
      modules: {
        engine: { kind: 'rust-crate', crate: 'craft-engine', responsibility: 'Pure decisions.', allow: ['Cargo.toml', 'src/**/*.rs'], dependsOn: [], externalDependencies: ['serde'], shipped: true },
        application: { kind: 'rust-crate', crate: 'craft-application', responsibility: 'Use cases.', allow: ['Cargo.toml', 'src/**/*.rs'], dependsOn: ['engine'], shipped: true },
        hosts: { kind: 'host-adapter', responsibility: 'Host tables.', allow: ['*.mjs'], maxFileLines: 5, dependsOn: [], shipped: false },
        packages: { kind: 'content', responsibility: 'Guidance.', allow: ['**/*.md'], dependsOn: [], shipped: true },
        tooling: { kind: 'build-tooling', responsibility: 'Build.', allow: ['*.mjs'], dependsOn: '*', shipped: false },
        tests: { kind: 'tests', responsibility: 'Tests.', allow: ['**/*.mjs'], dependsOn: '*', shipped: false },
        docs: { kind: 'docs', responsibility: 'Documentation.', allow: ['**/*.md'], dependsOn: [], shipped: true }
      }
    },
    '.gitignore': 'dist/\n',
    'Cargo.toml': '[workspace]\nmembers = ["engine", "application"]\n\n[workspace.lints.rust]\nunsafe_code = "forbid"\n',
    'engine/Cargo.toml': '[package]\nname = "craft-engine"\n\n[dependencies]\nserde.workspace = true\n\n[lints]\nworkspace = true\n',
    'engine/src/lib.rs': 'pub fn decide() -> u8 {\n    1\n}\n',
    'application/Cargo.toml': '[package]\nname = "craft-application"\n\n[dependencies]\ncraft-engine = { path = "../engine" }\n\n[lints]\nworkspace = true\n',
    'application/src/main.rs': 'fn main() {}\n',
    'hosts/table.mjs': 'export const table = 1;\n',
    'packages/a/README.md': '# A\n',
    'tooling/build.mjs': "import { table } from '../hosts/table.mjs';\nexport const build = table;\n",
    'docs/adr/0002.md': ['Status: Accepted', '', '## Module map', '', '| Module | Kind | Depends on | Shipped | Responsibility |', '|---|---|---|---|---|',
      adrRow('engine', 'rust-crate', '—', 'yes'), adrRow('application', 'rust-crate', 'engine', 'yes'), adrRow('hosts', 'host-adapter', '—', 'no'),
      adrRow('packages', 'content', '—', 'yes'), adrRow('tooling', 'build-tooling', '*', 'no'), adrRow('tests', 'tests', '*', 'no'), adrRow('docs', 'docs', '—', 'yes'), ''].join('\n')
  };
}

// A shipped content module that imports a host table (itself an A8 violation; the case checks A6).
function importTable(files, modules, name) {
  modules.packages.allow.push('*/*.mjs');
  files[`hosts/${name}.mjs`] ??= 'export const x = 1;\n';
  files['packages/a/hook.mjs'] = `import '../../hosts/${name}.mjs';\n`;
}

function check(t, mutate = () => {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-architecture-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const files = baseline();
  mutate(files, files['architecture.json'].modules);
  for (const [file, content] of Object.entries(files)) {
    if (content === null) continue;
    fs.mkdirSync(path.dirname(path.join(root, file)), { recursive: true });
    fs.writeFileSync(path.join(root, file), typeof content === 'string' ? content : `${JSON.stringify(content, null, 2)}\n`);
  }
  execFileSync('git', ['init', '-q'], { cwd: root });
  const result = checkArchitecture(root);
  return { root, result, rules: [...new Set(result.errors.map(error => error.rule))].sort() };
}

test('glob patterns distinguish one segment from nested paths', () => {
  assert.ok(glob('src/**/*.rs').test('src/lib.rs'));
  assert.ok(glob('src/**/*.rs').test('src/a/b.rs'));
  assert.ok(!glob('*/hooks/*.mjs').test('a/b/hooks/x.mjs'));
  assert.ok(glob('examples/**').test('examples/project/x.json'));
  assert.ok(!glob('*.json').test('a/b.json'));
});

test('the repository satisfies its own module map', () => {
  const { errors } = checkArchitecture(path.resolve('.'));
  assert.deepEqual(errors, []);
});

test('a consistent fixture passes every rule', t => {
  assert.deepEqual(check(t).result.errors, []);
});

const violations = {
  A1: [
    ['undeclared top-level module', files => { files['scratch/notes.md'] = 'x\n'; }],
    ['undeclared root file', files => { files['TODO.md'] = 'x\n'; }]
  ],
  A2: [
    ['JavaScript inside a crate', files => { files['engine/src/decide.mjs'] = 'export {};\n'; }],
    ['unexpected file type', files => { files['docs/diagram.txt'] = 'x\n'; }]
  ],
  A3: [
    ['ADR disagrees with the manifest', files => { files['docs/adr/0002.md'] = files['docs/adr/0002.md'].replace("| `packages/` | content | —", '| `packages/` | content | hosts'); }],
    ['unknown kind', (files, modules) => { modules.docs.kind = 'helpers'; }],
    ['decision not accepted', files => { files['docs/adr/0002.md'] = files['docs/adr/0002.md'].replace('Status: Accepted', 'Status: Proposed'); }],
    ['multi-sentence responsibility', (files, modules) => { modules.docs.responsibility = 'Docs. And more.'; }]
  ],
  A4: [
    ['engine depends on application', files => { files['engine/Cargo.toml'] = files['engine/Cargo.toml'].replace('serde.workspace = true', 'serde.workspace = true\ncraft-application = { path = "../application" }'); }],
    ['undeclared external crate', files => { files['engine/Cargo.toml'] = files['engine/Cargo.toml'].replace('serde.workspace = true', 'serde.workspace = true\ntokio = "1"'); }],
    ['workspace member is not a declared crate', files => { files['Cargo.toml'] = files['Cargo.toml'].replace('"application"]', '"application", "extra"]'); }],
    ['unsafe code allowed', files => { files['Cargo.toml'] = files['Cargo.toml'].replace('unsafe_code = "forbid"', ''); }]
  ],
  A5: [
    ['engine reads files', files => { files['engine/src/lib.rs'] = 'pub fn decide() -> bool {\n    std::fs::metadata("x").is_ok()\n}\n'; }],
    ['engine reads the clock', files => { files['engine/src/lib.rs'] = 'use std::time::SystemTime;\npub fn now() -> SystemTime {\n    SystemTime::now()\n}\n'; }]
  ],
  A6: [
    ['import of a private file', (files, modules) => { importTable(files, modules, 'private'); modules.packages.dependsOn = ['hosts']; modules.hosts.entryPoints = ['hosts/table.mjs']; }],
    ['import without a declared dependency', (files, modules) => { importTable(files, modules, 'table'); }],
    ['stale entry point', (files, modules) => { modules.hosts.entryPoints = ['hosts/table.mjs']; }]
  ],
  A7: [
    ['runtime JS kept alive only by a test', files => { files['hosts/dead.mjs'] = 'export const dead = 1;\n'; files['tests/dead.test.mjs'] = "import '../hosts/dead.mjs';\n"; }]
  ],
  A8: [
    ['JavaScript in a content module', (files, modules) => { modules.packages.allow.push('*/*.mjs'); files['packages/a/hook.mjs'] = "export const native = 'observe';\n"; }],
    ['host adapter with a module dependency', (files, modules) => { modules.hosts.dependsOn = ['packages']; }]
  ],
  A9: [
    ['generic module name', files => { files['engine/src/utils.rs'] = 'pub fn x() {}\n'; files['engine/src/lib.rs'] += 'mod utils;\n'; }],
    ['empty module', files => { files['engine/src/routing.rs'] = '\n'; }]
  ],
  A10: [
    ['fat host adapter', (files, modules) => { files['hosts/table.mjs'] += 'const a = 1;\nconst b = 2;\nconst c = 3;\nconst d = 4;\nconst e = 5;\n'; }]
  ],
  A11: [
    ['unshipped module in an export', files => { files['dist/agents/tests/x.mjs'] = 'export {};\n'; }]
  ]
};

for (const [rule, cases] of Object.entries(violations)) for (const [name, mutate] of cases) {
  test(`${rule} rejects: ${name}`, t => {
    const { rules, result } = check(t, mutate);
    assert.ok(rules.includes(rule), `${rule} expected; got ${JSON.stringify(result.errors)}`);
  });
}

test('A5 ignores test-only filesystem use in the engine', t => {
  const { rules } = check(t, files => { files['engine/src/lib.rs'] += '\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn reads() {\n        let _ = std::fs::metadata(".");\n    }\n}\n'; });
  assert.deepEqual(rules, []);
});
