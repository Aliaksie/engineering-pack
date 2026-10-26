import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

// Module boundaries are enforced by `npm run architecture` (tests/architecture-rules.test.mjs).
const root = path.resolve('.');
function modules(directory) {
  return fs.readdirSync(directory,{ withFileTypes:true }).flatMap(entry => {
    const file = path.join(directory,entry.name);
    return entry.isDirectory() ? modules(file) : file.endsWith('.mjs') ? [file] : [];
  });
}
function imports(source) {
  return [...source.matchAll(/(?:from\s*|import\s*|import\(\s*)(['"])([^'"]+)\1/g)].map(match => match[2]);
}
const inDirectory = (file,directory) => file.startsWith(path.join(root,directory)+path.sep);

test('host tables cannot depend on build tooling, and retired runtime trees stay gone', () => {
  for (const file of modules(path.join(root,'hosts'))) for (const specifier of imports(fs.readFileSync(file,'utf8'))) {
    assert.ok(!inDirectory(path.resolve(path.dirname(file),specifier),'tooling'),`${file}: build dependency`);
  }
  for (const directory of ['runtime','adapters','scripts','cli','integrations','tools','platform/node']) {
    assert.equal(fs.existsSync(path.join(root,directory)),false,`obsolete ${directory} tree retained`);
  }
});

test('delivery policy uses lifecycle facts rather than host hook event names', () => {
  const source = fs.readFileSync(path.join(root,'engine/src/delivery.rs'),'utf8');
  assert.doesNotMatch(source,/before-tool|before-prompt|before-compact|after-compact|session-start|turn-end/);
});
