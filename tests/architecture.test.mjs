import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

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

test('engine depends only on portable engine modules and explicit observations', () => {
  for (const file of modules(path.join(root,'engine'))) {
    const source = fs.readFileSync(file,'utf8');
    for (const specifier of imports(source)) {
      assert.ok(specifier.startsWith('.'),`${file}: ${specifier}`);
      assert.ok(inDirectory(path.resolve(path.dirname(file),specifier),'engine'),`${file}: outward dependency ${specifier}`);
    }
    assert.doesNotMatch(source,/\b(?:process|Buffer)\.|node:|import\.meta|Date\.now\(|new Date\(\s*\)|Math\.random\(/,file);
  }
});

test('application orchestrates through the public platform and engine boundaries', () => {
  for (const file of modules(path.join(root,'application'))) {
    const source = fs.readFileSync(file,'utf8');
    for (const specifier of imports(source)) {
      assert.ok(!specifier.startsWith('node:'),`${file}: Node I/O in use case`);
      const target = path.resolve(path.dirname(file),specifier);
      if (inDirectory(target,'platform')) assert.equal(target,path.join(root,'platform/index.mjs'),`${file}: platform implementation detail`);
      if (inDirectory(target,'engine')) assert.equal(target,path.join(root,'engine/index.mjs'),`${file}: private engine import`);
    }
    assert.doesNotMatch(source,/\b(?:process|Buffer)\.|Date\.now\(|new Date\(\s*\)/,file);
  }
});

test('Node platform implementations never import use cases or decision policy', () => {
  for (const file of modules(path.join(root,'platform/node'))) for (const specifier of imports(fs.readFileSync(file,'utf8'))) {
    assert.ok(specifier.startsWith('node:') || inDirectory(path.resolve(path.dirname(file),specifier),'platform/node'),`${file}: upward platform dependency ${specifier}`);
  }
});

test('host runtime and external transports cannot depend on build tooling or private policy', () => {
  for (const directory of ['hosts','integrations','cli']) for (const file of modules(path.join(root,directory))) {
    for (const specifier of imports(fs.readFileSync(file,'utf8'))) {
      const target = path.resolve(path.dirname(file),specifier);
      assert.ok(!inDirectory(target,'tooling'),`${file}: build dependency`);
      if (inDirectory(target,'engine')) assert.equal(target,path.join(root,'engine/index.mjs'),`${file}: private engine dependency`);
      if (directory === 'hosts') assert.ok(!inDirectory(target,'integrations'),`${file}: host depends on external provider integration`);
    }
  }
  for (const directory of ['runtime','adapters','scripts','application/node']) assert.equal(fs.existsSync(path.join(root,directory)),false,`obsolete ${directory} tree retained`);
});

test('delivery policy uses lifecycle facts rather than host hook event names', () => {
  const source = fs.readFileSync(path.join(root,'engine/policy/context-delivery.mjs'),'utf8');
  assert.doesNotMatch(source,/before-tool|before-prompt|before-compact|after-compact|session-start|turn-end/);
});

test('only the application engine client imports engine implementation entry points', () => {
  for (const directory of ['application','platform','hosts','integrations','cli','tooling']) for (const file of modules(path.join(root,directory))) {
    for (const specifier of imports(fs.readFileSync(file,'utf8'))) {
      const target = path.resolve(path.dirname(file),specifier);
      if (inDirectory(target,'engine')) {
        assert.equal(file,path.join(root,'application/engine-client.mjs'),`${file}: bypasses JSON engine client`);
        assert.equal(target,path.join(root,'engine/index.mjs'),`${file}: private engine dependency`);
      }
    }
  }
  const client = fs.readFileSync(path.join(root,'application/engine-client.mjs'),'utf8');
  assert.deepEqual(imports(client),['../engine/index.mjs']);
});

test('portable telemetry contracts cannot depend on providers or runtime implementations', () => {
  const contract = fs.readFileSync(path.join(root,'application/telemetry/events.mjs'),'utf8');
  assert.deepEqual(imports(contract),[]);
  assert.doesNotMatch(contract,/AsyncLocalStorage|resourceSpans|scopeSpans|spanId|OTLP|OpenTelemetry|node:|process\./);
  for (const directory of ['engine','application']) for (const file of modules(path.join(root,directory))) {
    const source = fs.readFileSync(file,'utf8');
    assert.doesNotMatch(source,/@opentelemetry\//,file);
    if (file !== path.join(root,'application/telemetry/observe.mjs')) for (const specifier of imports(source)) {
      assert.ok(!specifier.includes('integrations/otel/'),`${file}: bypasses telemetry boundary`);
    }
    assert.doesNotMatch(source,/resourceSpans|scopeSpans|startTimeUnixNano/,file);
  }
  for (const file of modules(path.join(root,'integrations/otel'))) for (const specifier of imports(fs.readFileSync(file,'utf8'))) {
    assert.ok(inDirectory(path.resolve(path.dirname(file),specifier),'integrations/otel'),`${file}: provider mapping depends on core contract implementation`);
  }
});

test('Rust workspace keeps the engine, platform and application dependency direction', () => {
  const workspace = fs.readFileSync(path.join(root, 'Cargo.toml'), 'utf8');
  assert.match(workspace, /members = \["engine", "platform", "application"\]/);
  assert.match(workspace, /resolver = "3"/);
  assert.match(workspace, /unsafe_code = "forbid"/);
  const allowed = { engine: [], platform: ['engine'], application: ['engine', 'platform'] };
  for (const [name, dependencies] of Object.entries(allowed)) {
    const manifest = fs.readFileSync(path.join(root, name, 'Cargo.toml'), 'utf8');
    assert.match(manifest, new RegExp(`name = "craft-${name}"`));
    assert.match(manifest, /\[lints\]\nworkspace = true/);
    const local = [...manifest.matchAll(/path\s*=\s*"(\.\.[^"]+)"/g)].map(match => match[1]);
    assert.deepEqual(local.sort(), dependencies.map(dependency => `../${dependency}`).sort());
    assert.equal(fs.existsSync(path.join(root, name, 'native')), false);
  }
});
