import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import test from 'node:test';
import assert from 'node:assert/strict';
import { reuseRoutingHint, commitRoutingHint, invalidateRoutingHint } from '../application/context-delivery/hints.mjs';
import { validateProject } from '../application/project/configuration.mjs';

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'hint reuse '));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  return { routingRoot: root, session: 'fixture-session', host: 'codex', contextSupported: true,
    routing: {}, project: { formatVersion: 1, routing: { dedupeHints: true } } };
}
const result = { decision: 'allow', context: 'Selected routing hint; no raw prompt should be saved.' };

test('only emitted routing context is reused; scope/config/age/resume invalidate reuse', (t) => {
  const event = fixture(t);
  const unobserved = reuseRoutingHint(event, result, 100);
  commitRoutingHint(unobserved, false);
  const first = reuseRoutingHint(event, result, 101);
  assert.equal(first.result.context, result.context);
  commitRoutingHint(first, true);
  const quiet = reuseRoutingHint(event, result, 102);
  assert.equal(quiet.suppressed, true);
  assert.equal(quiet.result.context, undefined);
  assert.equal(quiet.result.decision, 'allow');
  const file = fs.readdirSync(path.join(event.routingRoot, '.craft/state')).find((file) => file.endsWith('.json'));
  const stored = fs.readFileSync(path.join(event.routingRoot, '.craft/state', file), 'utf8');
  assert.equal(stored.includes('Selected routing hint'), false);
  assert.deepEqual(Object.keys(JSON.parse(stored)).sort(), ['emittedAt', 'formatVersion', 'key']);
  for (const [changed, output, now] of [
    [{ ...event, project: { ...event.project, languages: ['python'] } }, result, 103],
    [event, { ...result, context: 'Different scoped routing hint' }, 104],
    [event, result, 15 * 60 * 1000 + 102]
  ]) {
    const next = reuseRoutingHint(changed, output, now);
    assert.ok(!next.suppressed);
    commitRoutingHint(next, false);
  }
  invalidateRoutingHint(event.routingRoot, event.session, event.host);
  const after = reuseRoutingHint(event, result, 105);
  assert.ok(!after.suppressed);
  commitRoutingHint(after, true);
  const racing = reuseRoutingHint(event, { ...result, context: 'New routing scope' }, 106);
  invalidateRoutingHint(event.routingRoot, event.session, event.host);
  commitRoutingHint(racing, true);
  const refreshed = reuseRoutingHint(event, { ...result, context: 'New routing scope' }, 107);
  assert.ok(!refreshed.suppressed, 'a late pre-compaction commit must not defeat invalidation');
  commitRoutingHint(refreshed, true);
});

test('reuse is opt-in and never suppresses unsupported, unrelated, or busy deliveries', (t) => {
  const event = fixture(t);
  for (const changed of [{ ...event, contextSupported: false }, { ...event, session: undefined },
    { ...event, project: { formatVersion: 1 } }, { ...event, routing: undefined }]) {
    assert.deepEqual(reuseRoutingHint(changed, result), { result });
  }
  assert.ok(!fs.existsSync(path.join(event.routingRoot, '.craft/state')));
  const first = reuseRoutingHint(event, result, 100);
  const busy = reuseRoutingHint(event, result, 101);
  assert.equal(busy.result.context, result.context);
  assert.ok(!busy.ticket);
  commitRoutingHint(first, true);
  assert.equal(validateProject({ formatVersion: 1, routing: { dedupeHints: true } }).routing.dedupeHints, true);
  assert.throws(() => validateProject({ formatVersion: 1, routing: { dedupeHints: 'yes' } }), /dedupeHints/);
});

test('malformed/symlinked state emits the original advisory hint and releases owned locks', (t) => {
  const event = fixture(t);
  const first = reuseRoutingHint(event, result, 100); commitRoutingHint(first, true);
  const dir = path.join(event.routingRoot, '.craft/state');
  const file = path.join(dir, fs.readdirSync(dir).find((file) => file.endsWith('.json')));
  fs.writeFileSync(file, 'invalid');
  const malformed = reuseRoutingHint(event, result, 101);
  assert.equal(malformed.result.context, result.context);
  assert.match(malformed.diagnostic, /unavailable/);
  assert.ok(!fs.existsSync(`${file}.lock`));
  fs.unlinkSync(file);
  const missing = path.join(event.routingRoot, 'absent.json');
  fs.symlinkSync(missing, file);
  assert.equal(reuseRoutingHint(event, result, 102).result.context, result.context);
  assert.equal(fs.existsSync(missing), false);
});
