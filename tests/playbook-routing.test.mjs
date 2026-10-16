import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { route } from '../runtime/router.mjs';
import { catalog } from '../runtime/catalog.mjs';

const select = (file, config = {}, intent = 'implement', risks = []) => route({ intent, files: [file], project: { formatVersion: 1, ...config }, risks });

test('TypeScript browser and Node contexts remain independent of React', () => {
  const browser = select('src/main.ts', { runtimes: ['browser'], layers: ['ui'] });
  assert.ok(browser.loading.specializations.includes('runtime:browser'));
  assert.ok(!browser.loading.specializations.includes('runtime:node'));
  assert.ok(!browser.skills.includes('craft-engineering:backend-engineering'));
  const node = select('src/main.ts', { runtimes: ['node'], layers: ['backend'] });
  assert.ok(node.skills.includes('craft-engineering:backend-foundations'));
  assert.ok(!node.skills.includes('craft-engineering:backend-engineering'));
  assert.ok(!node.skills.includes('craft-engineering:react-engineering'));
  const unknown = select('src/main.ts', { layers: ['backend'] });
  assert.ok(!unknown.loading.specializations.includes('runtime:node'));
});

test('Kotlin infers JVM without Spring; Spring requires an explicit declaration', () => {
  const kotlin = select('src/Main.kt');
  assert.ok(kotlin.loading.specializations.includes('language:kotlin'));
  assert.ok(kotlin.loading.specializations.includes('runtime:jvm'));
  assert.ok(!kotlin.loading.specializations.includes('framework:spring'));
  assert.ok(kotlin.loading.evidence.some((entry) => entry.source === 'language:kotlin' && entry.inferred));
  const java = select('src/Main.java', { frameworks: ['spring'] });
  assert.ok(java.loading.specializations.includes('framework:spring'));
  assert.ok(java.loading.unavailable.some(({ id }) => id === 'framework:spring'));
  assert.equal(java.loading.documentationQuestions.length, 1);
});

test('React accessibility review selects the risk reference and review workflow', () => {
  const result = select('src/Main.tsx', { frameworks: ['react'] }, 'review', ['accessibility']);
  assert.equal(result.loading.workflow, 'workflow:review');
  assert.ok(result.loading.practices.includes('practice:accessibility'));
  assert.ok(result.references.includes('craft-engineering:react-engineering/references/accessibility.md'));
});

test('Bedrock TypeScript agent loads its provider only; story loads no stack', () => {
  const config = { layers: ['agent'], providers: ['bedrock'] };
  const agent = select('agent.ts', config);
  assert.deepEqual(new Set(agent.references), new Set(['craft-engineering:foundations/references/languages/typescript.md', 'craft-engineering:design-agent/references/bedrock.md']));
  assert.ok(!agent.skills.includes('craft-engineering:backend-engineering'));
  const story = select('agent.ts', config, 'story');
  assert.deepEqual(story.loading.specializations, []);
  assert.deepEqual(story.references, []);
  assert.ok(story.skills.every((id) => id.startsWith('craft-stories:')));
});

test('mixed module evidence cannot combine unrelated providers and layers', () => {
  const result = route({ intent: 'review', files: ['ui/main.tsx', 'worker/main.py'], project: { formatVersion: 1, modules: [
    { name: 'ui', path: 'ui', layers: ['ui'], providers: ['bedrock'], frameworks: ['react'] },
    { name: 'worker', path: 'worker', layers: ['agent'] }
  ] } });
  assert.deepEqual(new Set(result.references), new Set(['craft-engineering:foundations/references/languages/python.md', 'craft-engineering:foundations/references/languages/typescript.md']));
  assert.ok(!result.loading.specializations.includes('provider:bedrock'));
});

test('investigation remains available and routing never claims to execute checks', () => {
  const result = select('src/Main.java', {}, 'investigate');
  assert.equal(result.loading.workflow, 'workflow:investigate');
  assert.equal(result.execution.mode, 'main');
  assert.deepEqual(result.operations, []);
  assert.match(result.authorization, /route-only/);
});

test('catalog identities are unique and all load targets exist', () => {
  assert.equal(new Set(catalog.entries.map(({ id }) => id)).size, catalog.entries.length);
  for (const entry of catalog.entries) {
    assert.match(entry.id, /^(workflow|practice|language|runtime|framework|provider|layer):[a-z][a-z0-9-]*$/);
    for (const target of [...entry.skills, ...entry.references]) {
      const [pkg, local] = target.split(':');
      const location = entry.skills.includes(target) ? `skills/${local}/SKILL.md` : `skills/${local}`;
      assert.ok(fs.existsSync(path.join('packages', pkg, location)), target);
    }
  }
});

test('runtime identity does not require a backend layer; missing runtime is visible', () => {
  const node = select('src/main.ts', { runtimes: ['node'] });
  assert.ok(node.loading.specializations.includes('runtime:node'));
  assert.ok(!node.skills.includes('craft-engineering:backend-engineering'));
  assert.ok(select('src/main.ts').loading.missingContext.some((message) => message.includes('Runtime is unspecified')));
});

test('nested modules do not lend their file evidence to a parent module', () => {
  const result = route({ intent: 'implement', files: ['src/main.py', 'src/ui/main.tsx'], project: { formatVersion: 1, modules: [
    { name: 'parent', path: 'src', layers: ['backend'], runtimes: ['node'] },
    { name: 'ui', path: 'src/ui', layers: ['ui'], runtimes: ['browser'] }
  ] } });
  const language = result.loading.selections.find(({ id }) => id === 'language:typescript');
  assert.ok(language.evidence.every((item) => item.source !== 'src/main.py'));
  const ui = result.loading.selections.find(({ id }) => id === 'layer:ui');
  assert.ok(ui.evidence.every((item) => item.source !== 'module:parent'));
});

test('failure-path practice is selected independently of language, Node and provider', () => {
  for (const file of ['src/Main.java', 'src/main.py', 'src/main.ts']) {
    const result = select(file, {}, 'implement', ['timeout']);
    assert.ok(result.loading.practices.includes('practice:error-handling'));
    assert.ok(result.references.includes('craft-engineering:backend-engineering/references/error-handling.md'));
    assert.ok(!result.skills.includes('craft-engineering:backend-engineering'));
    assert.ok(!result.loading.specializations.includes('provider:bedrock'));
  }
  assert.ok(!select('src/Main.java').loading.practices.includes('practice:error-handling'));
  assert.deepEqual(select('src/main.ts', {}, 'story', ['retry']).references, []);
});

test('investigation and reliability references are risk-selected without stack assumptions', () => {
  const investigation = select('src/main.py', {}, 'investigate', ['debugging']);
  assert.ok(investigation.references.includes('craft-engineering:testing-engineering/references/systematic-debugging.md'));
  const reliability = select('src/Main.java', {}, 'verify', ['flaky-test']);
  assert.ok(reliability.references.includes('craft-engineering:testing-engineering/references/test-data-and-flakiness.md'));
  assert.ok(!select('src/Main.java', {}, 'verify').references.includes('craft-engineering:testing-engineering/references/test-data-and-flakiness.md'));
  assert.deepEqual(select('src/main.py', {}, 'story', ['debugging', 'flaky-test']).references, []);
  assert.ok(investigation.loading.selections.every((entry) => !Object.hasOwn(entry, 'value')),
    'authoring value metadata must not inflate the execution loading plan');
});

test('backend decision references follow risk independently of runtime and do not load on story work', () => {
  const risks = ['concurrency', 'audit-trail', 'resource-lifetime', 'performance', 'consistency', 'infrastructure'];
  const expected = ['async-patterns', 'observability-logging', 'service-layers', 'performance-efficiency']
    .map((name) => `craft-engineering:backend-engineering/references/${name}.md`);
  expected.push('craft-engineering:backend-engineering/references/distributed-systems.md',
    'craft-engineering:foundations/references/infrastructure.md');
  for (const file of ['src/Main.kt', 'src/main.py', 'src/main.ts', 'deploy/resources.config']) {
    const result = select(file, {}, 'review', risks);
    assert.ok(expected.every((reference) => result.references.includes(reference)));
    assert.ok(!result.skills.includes('craft-engineering:backend-engineering'));
    assert.ok(!result.loading.specializations.includes('runtime:node'));
  }
  const plain = select('src/main.ts', { runtimes: ['node'], layers: ['backend'] });
  assert.ok(expected.every((reference) => !plain.references.includes(reference)));
  assert.ok(!plain.loading.practices.includes('practice:node-backend'));
  assert.deepEqual(select('src/main.py', {}, 'story', risks).references, []);
});


test('locate selects bounded lookup without loading declared stack or risk practices', () => {
  const project = { formatVersion: 1, languages: ['typescript'], runtimes: ['node'],
    frameworks: ['react'], layers: ['agent', 'ui'], providers: ['bedrock'],
    modules: [{ name: 'agent', path: 'agent', layers: ['agent'],
      skills: ['craft-engineering:design-agent'] }] };
  const result = route({ intent: 'locate', files: ['agent/main.ts'], project,
    risks: ['concurrency', 'auth', 'infrastructure'] });
  assert.equal(result.loading.workflow, 'workflow:locate');
  assert.deepEqual(result.skills, ['craft-engineering:locate-code']);
  assert.deepEqual(result.references, []);
  assert.deepEqual(result.loading.practices, []);
  assert.deepEqual(result.loading.specializations, []);
  assert.deepEqual(result.execution, { mode: 'tool', role: 'locator' });
  assert.deepEqual(result.operations, []);
});

test('planning surfaces design criteria without loading the full foundations skill or story guidance', () => {
  const config = { layers: ['backend'], runtimes: ['node'] };
  const reference = 'craft-engineering:foundations/references/design-principles.md';
  const plan = select('src/service.ts', config, 'plan');
  assert.equal(plan.loading.workflow, 'workflow:plan');
  assert.ok(plan.references.includes(reference));
  assert.ok(plan.references.includes('craft-engineering:foundations/references/languages/typescript.md'));
  assert.ok(!plan.skills.includes('craft-engineering:foundations'));
  assert.ok(!plan.skills.some((id) => id.startsWith('craft-stories:')));
  const story = select('src/service.ts', config, 'story', ['concurrency']);
  assert.deepEqual(story.references, []);
  assert.deepEqual(story.loading.specializations, []);
  assert.ok(story.skills.every((id) => id.startsWith('craft-stories:')));
});
