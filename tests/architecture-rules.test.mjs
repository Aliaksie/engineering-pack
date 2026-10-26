import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import assert from 'node:assert/strict';
import test from 'node:test';
import { checkArchitecture, glob } from '../tooling/architecture.mjs';

const adrRow = (name, kind, deps, legacy, shipped) => `| \`${name}/\` | ${kind} | ${deps} | ${legacy} | ${shipped} | Fixture |`;
function baseline() {
  return {
    'architecture.json': {
      formatVersion: 1, decision: 'docs/adr/0002.md', rootFiles: ['.gitignore', 'Cargo.toml', 'architecture.json'],
      modules: {
        engine: { kind: 'rust-crate', crate: 'craft-engine', responsibility: 'Pure decisions.', allow: ['Cargo.toml', 'src/**/*.rs'], dependsOn: [], externalDependencies: ['serde'], shipped: true },
        application: { kind: 'rust-crate', crate: 'craft-application', responsibility: 'Use cases.', allow: ['Cargo.toml', 'src/**/*.rs'], dependsOn: ['engine'], shipped: true,
          legacy: { retireBy: 'W4', allow: ['**/*.mjs'], jsLines: 1, dependsOn: [], entryPoints: ['application/index.mjs'] } },
        hosts: { kind: 'host-adapter', responsibility: 'Host wire output.', allow: ['*.mjs'], jsLines: 2, maxFileLines: 5, dependsOn: ['application'], shipped: true, runtimeRoots: ['hosts/*.mjs'] },
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
    'application/index.mjs': 'export const run = () => 1;\n',
    'hosts/hook.mjs': "import { run } from '../application/index.mjs';\nrun();\n",
    'docs/adr/0002.md': ['Status: Accepted', '', '## Module map', '', '| Module | Kind | Depends on | Legacy | Shipped | Responsibility |', '|---|---|---|---|---|---|',
      adrRow('engine', 'rust-crate', '—', '—', 'yes'), adrRow('application', 'rust-crate', 'engine', '—', 'yes'), adrRow('hosts', 'host-adapter', 'application', '—', 'yes'),
      adrRow('tests', 'tests', '*', '—', 'no'), adrRow('docs', 'docs', '—', '—', 'yes'), ''].join('\n')
  };
}

function check(t, mutate = () => {}, options) {
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
  const result = checkArchitecture(root, options);
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
    ['JavaScript inside a crate without legacy permission', files => { files['engine/src/decide.mjs'] = 'export {};\n'; }],
    ['unexpected file type', files => { files['docs/diagram.txt'] = 'x\n'; }]
  ],
  A3: [
    ['ADR disagrees with the manifest', files => { files['docs/adr/0002.md'] = files['docs/adr/0002.md'].replace("| `hosts/` | host-adapter | application", '| `hosts/` | host-adapter | —'); }],
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
    ['import of a private file', files => { files['application/private.mjs'] = 'export const x = 1;\n'; files['hosts/hook.mjs'] += "import { x } from '../application/private.mjs';\n"; }],
    ['import without a declared dependency', (files, modules) => { modules.hosts.dependsOn = []; }]
  ],
  A7: [
    ['runtime JS kept alive only by a test', files => { files['application/dead.mjs'] = 'export const dead = 1;\n'; files['tests/dead.test.mjs'] = "import '../application/dead.mjs';\n"; }]
  ],
  A8: [
    ['runtime JS grows', files => { files['application/index.mjs'] += 'export const more = 2;\n'; }],
    ['budget left higher than the code', (files, modules) => { modules.application.legacy.jsLines = 9; }],
    ['host adapter keeps dependencies after migration', (files, modules) => { delete modules.application.legacy; files['application/index.mjs'] = null; files['hosts/hook.mjs'] = 'export {};\n'; modules.hosts.jsLines = 1; }]
  ],
  A9: [
    ['generic module name', files => { files['engine/src/utils.rs'] = 'pub fn x() {}\n'; files['engine/src/lib.rs'] += 'mod utils;\n'; }],
    ['empty module', files => { files['engine/src/routing.rs'] = '\n'; }]
  ],
  A10: [
    ['fat host adapter', (files, modules) => { files['hosts/hook.mjs'] += 'const a = 1;\nconst b = 2;\nconst c = 3;\nconst d = 4;\n'; modules.hosts.jsLines = 6; }]
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

test('budget updates only lower the recorded line count', t => {
  const { root, result } = check(t, (files, modules) => { modules.application.legacy.jsLines = 9; }, { updateBudgets: true });
  assert.deepEqual(result.errors, []);
  assert.equal(JSON.parse(fs.readFileSync(path.join(root, 'architecture.json'), 'utf8')).modules.application.legacy.jsLines, 1);
  const grown = check(t, files => { files['application/index.mjs'] += 'export const more = 2;\n'; }, { updateBudgets: true });
  assert.ok(grown.rules.includes('A8'));
  assert.equal(JSON.parse(fs.readFileSync(path.join(grown.root, 'architecture.json'), 'utf8')).modules.application.legacy.jsLines, 1);
});
