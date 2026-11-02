import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const script = fileURLToPath(new URL('../evals/image/craft-rewards.sh', import.meta.url));
const tools = ['bash', 'jq'].every(tool => spawnSync(tool, ['--version']).status === 0);

const event = (name, operationId, extra = {}) => ({
  schemaVersion: 1, name, kind: 'operation', timestamp: '2026-11-02T18:59:28.039Z',
  sessionId: 's', traceId: 't', operationId, packVersion: '0.0.2', outcome: 'success', ...extra,
});

function rewards(events) {
  const dir = mkdtempSync(join(tmpdir(), 'craft-rewards-'));
  try {
    const telemetry = join(dir, 'repo', '.craft', 'state', 'telemetry');
    mkdirSync(telemetry, { recursive: true });
    writeFileSync(join(telemetry, 'events.jsonl'), events.map(e => JSON.stringify(e)).join('\n') + '\n');
    writeFileSync(join(dir, 'expected.json'), '{}');
    const run = spawnSync('bash', [script, join(dir, 'repo'), join(dir, 'expected.json'), dir], { encoding: 'utf8' });
    assert.equal(run.status, 0, run.stderr);
    return JSON.parse(run.stdout);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test('each craft locate run counts as one lookup, whatever it records beneath it', { skip: !tools && 'needs bash and jq' }, () => {
  const result = rewards([
    // Structural lookup: the command alone.
    event('cli.command', 'a', { attributes: { command: 'locate' } }),
    // Literal lookup: the command and its child discovery.
    event('cli.command', 'b', { attributes: { command: 'locate' } }),
    event('discovery.completed', 'c', { parentOperationId: 'b' }),
    // Discovery outside a locate command still counts.
    event('discovery.completed', 'd'),
    // Other commands are not lookups.
    event('cli.command', 'e', { attributes: { command: 'checkpoint' } }),
  ]);
  assert.equal(result.craft_lookups, 3);
});

test('no lookup recorded scores zero', { skip: !tools && 'needs bash and jq' }, () => {
  assert.equal(rewards([event('routing.completed', 'a')]).craft_lookups, 0);
});

test('a prompt-time lookup counts once and as an answer only when delivered', { skip: !tools && 'needs bash and jq' }, () => {
  const result = rewards([
    event('host.hook', 'h1'),
    // Answered in the hint; its gap search is a child of the lookup.
    event('discovery.completed', 'p1', { parentOperationId: 'h1', attributes: { complete: true, evidenceCount: 4 } }),
    event('discovery.completed', 'g1', { parentOperationId: 'p1' }),
    // Abstained: still a lookup, not an answer.
    event('host.hook', 'h2'),
    event('discovery.completed', 'p2', { parentOperationId: 'h2', attributes: { complete: false, evidenceCount: 0 } }),
  ]);
  assert.equal(result.craft_lookups, 2);
  assert.equal(result.craft_prompt_answers, 1);
});
