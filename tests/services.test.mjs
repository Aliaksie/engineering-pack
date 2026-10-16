import test from 'node:test';
import assert from 'node:assert/strict';
import { prepareServices } from '../runtime/services.mjs';
import { planOperation } from '../runtime/bindings.mjs';

const operation = (summary, name) => summary.services.flatMap((service) => service.operations).find((entry) => entry.operation === name);

test('service preparation retains local Git, makes missing bindings explicit, and requires no discovery', () => {
  const project = { formatVersion: 1 };
  const before = structuredClone(project);
  const result = prepareServices(project);
  assert.deepEqual(project, before);
  assert.deepEqual(result, prepareServices(project));
  assert.equal(result.services[0].source, 'local-default');
  assert.equal(operation(result, 'git.status').command, 'git');
  assert.equal(operation(result, 'git.status').availability, 'unverified');
  assert.deepEqual(operation(result, 'git.diff').optionalParameters, ['base', 'head']);
  assert.match(operation(result, 'task.read').missing, /missing tasks binding/);
  assert.match(operation(result, 'change.read').missing, /missing codehost binding/);
  assert.match(operation(result, 'git.push').missing, /no mapping/);
  assert.equal(result.reuse.prepare, 'once-per-session-or-first-service-need');
  assert.ok(result.reuse.refreshOn.includes('account-or-scope-change'));
  assert.ok(result.reuse.refreshOn.includes('relevant-operation-failure'));
});

test('built-in codehost plans require explicit repository scope and report host names without guessing accounts', () => {
  for (const [provider, command] of [['github', 'gh'], ['gitlab', 'glab']]) {
    const project = { formatVersion: 1, services: { codehost: { provider, transport: 'cli', command, host: 'code.example.test', repository: 'team/project' } } };
    const summary = prepareServices(project, { availableCommands: [command] });
    const service = summary.services.find((entry) => entry.service === 'codehost');
    assert.deepEqual(service.scope, { host: 'code.example.test', repository: 'team/project', account: 'unverified' });
    for (const name of ['change.read', 'change.diff']) {
      const entry = operation(summary, name);
      assert.equal(entry.mapping, 'builtin');
      assert.equal(entry.availability, 'observed-available');
      assert.deepEqual(entry.requiredParameters, ['id']);
      assert.equal(entry.command, planOperation(project, name, { id: 7 }).command);
      assert.equal('args' in entry, false);
    }
    delete project.services.codehost.repository;
    assert.match(operation(prepareServices(project), 'change.read').missing, /repository must be explicit/);
  }
});

test('explicit CLI and MCP mappings reveal parameters and exact names without exposing argument values', () => {
  const project = { formatVersion: 1, services: {
    tasks: { provider: 'consumer-tracker', transport: 'mcp', host: 'tasks.example.test', operations: {
      'task.read': { tool: 'mcp__tracker__read', arguments: { nested: [{ key: '{id}', project: 'private-project', filter: 'owner={owner}' }], token: 'do-not-echo' } },
      'task.comment': { tool: 'mcp__tracker__comment', arguments: { id: '{id}', body: '{body}' } }
    } },
    git: { provider: 'git', transport: 'cli', command: 'git-wrapper', operations: {
      'git.commit': { args: ['commit', '-m', '{message}', '--author=private-identity'] }
    } }
  }, permissions: { writeOperations: ['task.comment'] } };
  const summary = prepareServices(project, { availableCommands: [], availableMcpTools: ['mcp__tracker__read'] });
  const read = operation(summary, 'task.read');
  assert.deepEqual(read.requiredParameters, ['id', 'owner']);
  assert.equal(read.tool, 'mcp__tracker__read');
  assert.equal(read.availability, 'observed-available');
  assert.equal(operation(summary, 'task.comment').availability, 'not-observed');
  assert.equal(operation(summary, 'task.comment').projectPolicy, 'enabled');
  assert.equal(operation(summary, 'task.comment').authorization, 'explicit-user-or-job-approval-required');
  assert.equal(operation(summary, 'git.commit').projectPolicy, 'disabled');
  assert.equal(operation(summary, 'git.commit').availability, 'not-observed');
  assert.deepEqual(operation(summary, 'git.commit').requiredParameters, ['message']);
  assert.equal(operation(summary, 'task.search').mapping, 'missing');
  assert.equal(operation(prepareServices(project), 'task.read').availability, 'unverified');
  for (const secret of ['do-not-echo', 'private-project', 'private-identity']) assert.equal(JSON.stringify(summary).includes(secret), false);
});

test('provider and tool visibility do not imply identity across different configured service scopes', () => {
  const binding = { provider: 'github', transport: 'cli', command: 'gh' };
  const first = prepareServices({ formatVersion: 1, services: { codehost: { ...binding, host: 'one.example.test', repository: 'team/a' } } }, { availableCommands: ['gh'] });
  const second = prepareServices({ formatVersion: 1, services: { codehost: { ...binding, host: 'two.example.test', repository: 'team/b' } } }, { availableCommands: ['gh'] });
  assert.notDeepEqual(first.services[1].scope, second.services[1].scope);
  assert.equal(first.services[1].scope.account, 'unverified');
  assert.equal(second.services[1].scope.account, 'unverified');
  assert.match(first.reuse.limits, /authorization/);
});

test('service preparation validates project and supplied inventories without executing commands', () => {
  assert.throws(() => prepareServices({ formatVersion: 2 }), /formatVersion/);
  assert.throws(() => prepareServices({ formatVersion: 1 }, { availableCommands: 'git' }), /array/);
  assert.throws(() => prepareServices({ formatVersion: 1 }, { availableMcpTools: [''] }), /array/);
  const project = { formatVersion: 1, services: { tasks: { provider: 'unknown', transport: 'cli', command: 'nonexistent; never-execute-this', operations: { 'task.read': { args: ['{id}'] } } } } };
  const entry = operation(prepareServices(project), 'task.read');
  assert.equal(entry.command, 'nonexistent; never-execute-this');
  assert.equal(entry.availability, 'unverified');
});
