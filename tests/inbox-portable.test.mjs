import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { readConfig } from '../tools/craft-review-inbox/lib/config.mjs';
import { checkout, mark, recordPosted, scan, tick } from '../tools/craft-review-inbox/lib/inbox.mjs';
import { atomicWrite, jsonFile, run, saveJson, withLock } from '../tools/craft-review-inbox/lib/io.mjs';
import { execute } from '../tools/craft-review-inbox/bin/inbox.mjs';
import { lintScripts } from '../scripts/lint-plugin-scripts.mjs';

const NOW = '2026-10-16T12:00:00Z';
const HEAD = 'a'.repeat(40);
const connection = (nodes = [], hasNextPage = false) => ({ nodes, pageInfo: { hasNextPage } });
const pr = (number = 1) => ({ number, title: 'Title | unicode λ\r\nsecond line', url: `https://example.test/org/api/pull/${number}`, isDraft: false, headRefOid: HEAD, baseRefName: 'main', headRefName: 'feature', author: { login: 'author' }, commits: { nodes: [{ commit: { oid: HEAD, committedDate: '2026-10-01T00:00:00Z' } }] }, reviews: connection(), comments: connection(), reviewThreads: connection() });
const response = (nodes, hasNextPage = false, endCursor = null) => JSON.stringify({ data: { repository: { pullRequests: { nodes, pageInfo: { hasNextPage, endCursor } } } } });
function fixture(t) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'inbox space λ ')));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const file = path.join(root, '.pr-loop/config.json');
  saveJson(file, { formatVersion: 1, host: 'example.test', owner: 'org', me: 'reviewer', repos: { api: 'api' } });
  return readConfig({ cwd: root });
}
const stored = (c, n = 1) => path.join(c.state, 'api', `pr_${n}.json`);

test('JSON config discovers parents, preserves explicit paths and never executes shell config', (t) => {
  const c = fixture(t); fs.mkdirSync(path.join(c.root, 'nested'));
  assert.equal(readConfig({ cwd: path.join(c.root, 'nested') }).root, c.root);
  const raw = jsonFile(c.file); raw.artifacts = 'artifacts with λ'; saveJson(c.file, raw);
  assert.equal(readConfig({ config: c.file }).artifacts, path.join(c.root, raw.artifacts));
  for (const invalid of [{ repos: { '../escape': 'api' } }, { repos: { api: '--option' } }, { repos: { CON: 'api' } }, { repos: { 'name.': 'api' } }, { host: 'host/path' }, { tickHours: -1 }, { token: 'not accepted' }]) {
    saveJson(c.file, { ...raw, ...invalid }); assert.throws(() => readConfig({ config: c.file }));
  }
  fs.unlinkSync(c.file); atomicWrite(path.join(c.home, 'config.sh'), 'throw new Error("must not execute")');
  assert.throws(() => readConfig({ cwd: c.root }), /config.json/);
});

test('full pagination is fetched before closing absent PRs, and arguments stay structured', (t) => {
  const c = fixture(t); const calls = [];
  const result = scan(c, { now: NOW, exec: (command, args, options) => {
    calls.push({ command, args, options });
    return calls.length === 1 ? response([pr(1)], true, 'cursor with spaces; $(never)') : response([pr(2)]);
  } });
  assert.equal(result.complete, true); assert.equal(result.updated, 2);
  assert.ok(calls[1].args.includes('cursor=cursor with spaces; $(never)'));
  assert.equal(calls[0].options.env.GH_HOST, 'example.test');
  assert.equal(jsonFile(stored(c, 2)).status, 'NEW');
  scan(c, { now: NOW, exec: () => response([pr(2)]) });
  assert.equal(jsonFile(stored(c, 1)).status, 'CLOSED_OR_MERGED');
  assert.match(fs.readFileSync(path.join(c.home, 'ledger.md'), 'utf8'), /Title \\\| unicode λ second line/);
});

test('partial, malformed, truncated and failed scans preserve previous state and freshness', (t) => {
  const c = fixture(t);
  scan(c, { now: NOW, exec: () => response([pr()]) });
  const before = fs.readFileSync(stored(c), 'utf8');
  const marker = fs.readFileSync(path.join(c.state, '.last-scan.json'), 'utf8');
  const nested = pr(); nested.reviews = connection([], true);
  for (const exec of [() => '{bad', () => JSON.stringify({ errors: [{ message: 'unavailable' }] }), () => response([nested]), () => { throw new Error('unavailable'); }]) {
    assert.equal(scan(c, { now: '2026-10-17T12:00:00Z', exec }).complete, false);
    assert.equal(fs.readFileSync(stored(c), 'utf8'), before);
    assert.equal(fs.readFileSync(path.join(c.state, '.last-scan.json'), 'utf8'), marker);
    assert.match(fs.readFileSync(path.join(c.home, 'INBOX.md'), 'utf8'), /Scan incomplete/);
  }
  let count = 0;
  const result = scan(c, { exec: () => ++count === 1 ? response([pr(2)], true, 'next') : (() => { throw new Error('page failed'); })() });
  assert.equal(result.complete, false);
  assert.equal(fs.existsSync(stored(c, 2)), false);
  assert.equal(fs.readFileSync(stored(c), 'utf8'), before);
});

test('failed tick retries; successful tick skips only within its actual freshness window', (t) => {
  const c = fixture(t); let calls = 0;
  assert.equal(tick(c, { now: NOW, exec: () => { calls++; throw new Error('offline'); } }).complete, false);
  assert.equal(fs.existsSync(path.join(c.state, '.last-scan.json')), false);
  tick(c, { now: NOW, exec: () => { calls++; return response([]); } });
  assert.equal(tick(c, { now: NOW, exec: () => assert.fail('must not scan fresh data') }).skipped, true);
  assert.equal(calls, 2);
  assert.throws(() => tick(c, { hours: NaN }), /invalid tick/);
});

test('mark pins an explicitly reviewed head; posted text is explicit, portable and local', (t) => {
  const c = fixture(t); scan(c, { now: NOW, exec: () => response([pr()]) });
  const artifact = path.join(c.artifacts, 'api/pr_1/pr-comment.md');
  atomicWrite(artifact, 'Original\r\n\r\nSuggested verdict: COMMENT\r\n');
  assert.throws(() => mark(c, 'api', 1, 'reviewed', { head: 'b'.repeat(40) }), /exact reviewed/);
  mark(c, 'api', 1, 'reviewed', { head: HEAD, now: NOW, verdict: 'COMMENT' });
  assert.equal(jsonFile(stored(c)).reviewed_head_sha, HEAD);
  assert.throws(() => recordPosted(c, 'api', 1), /requires/);
  const result = recordPosted(c, 'api', 1, { text: 'Actual edit\r\nSuggested verdict: COMMENT\r\n', now: NOW });
  assert.match(fs.readFileSync(result.sample, 'utf8'), /Actual edit/);
  assert.ok(!fs.readFileSync(result.sample, 'utf8').includes('Suggested verdict'));
  assert.match(fs.readFileSync(path.join(c.home, 'voice/EDITS.md'), 'utf8'), /Original/);
  assert.equal(jsonFile(stored(c)).posted_at, NOW);
  assert.equal(jsonFile(stored(c)).local_event_pending_scan, 'posted');
});

test('locks, malformed state and symlinked destinations fail without overwriting owned data', (t) => {
  const c = fixture(t);
  assert.throws(() => withLock(c, () => withLock(c, () => {})), /busy/);
  atomicWrite(path.join(c.home, 'bad.json'), '{bad');
  assert.throws(() => jsonFile(path.join(c.home, 'bad.json')));
  assert.equal(fs.existsSync(path.join(c.home, '.writer.lock')), false);
  // File symlinks may require privileges on Windows; all platforms still exercise lock/parse failure above.
  if (process.platform !== 'win32') {
    const outside = path.join(c.root, 'outside'); atomicWrite(outside, 'keep');
    const link = path.join(c.home, 'link'); fs.symlinkSync(outside, link);
    assert.throws(() => atomicWrite(link, 'replace'), /symlink/);
    assert.equal(fs.readFileSync(outside, 'utf8'), 'keep');
  }
});

test('checkout refuses moved heads and removes only its own newly-created directory', (t) => {
  const c = fixture(t); scan(c, { now: NOW, exec: () => response([pr()]) });
  const existing = path.join(c.scratch, 'existing/keep.txt'); atomicWrite(existing, 'keep');
  const exec = (_command, args) => args.includes('rev-parse') ? `${'b'.repeat(40)}\n` : '';
  assert.throws(() => checkout(c, 'api', 1, { exec }), /changed since scan/);
  assert.deepEqual(fs.readdirSync(c.scratch), ['existing']);
  assert.equal(fs.readFileSync(existing, 'utf8'), 'keep');
  const success = checkout(c, 'api', 1, { exec: (_command, args) => args.includes('rev-parse') ? `${HEAD}\n` : '' });
  assert.equal(success.head, HEAD); assert.equal(success.base, 'origin/main');
});

test('CLI rejects ignored options and returns a failing exit status for provider failure', (t) => {
  const c = fixture(t);
  assert.throws(() => execute(['mark', 'api', '1', 'note', '--unknown', 'value'], { cwd: c.root }), /unknown/);
  assert.throws(() => execute(['scan', 'extra'], { cwd: c.root }), /expects/);
  assert.throws(() => execute(['tick', '--hours'], { cwd: c.root }), /missing value/);
  const missing = spawnSync(process.execPath, [path.resolve('tools/craft-review-inbox/bin/inbox.mjs'), 'scan', '--config', path.join(c.root, 'missing.json')], { encoding: 'utf8' });
  assert.equal(missing.status, 2);
  const offline = spawnSync(process.execPath, [path.resolve('tools/craft-review-inbox/bin/inbox.mjs'), 'scan', '--config', c.file], { encoding: 'utf8', env: { ...process.env, PATH: c.root } });
  assert.equal(offline.status, 1, offline.stderr);
  assert.equal(JSON.parse(offline.stdout).complete, false);
  assert.equal(execute(['scan'], { cwd: c.root, exec: () => { throw new Error('offline'); } }).complete, false);
});

test('portability lint rejects shell executables outside hooks too', (t) => {
  const c = fixture(t); const file = path.join(c.root, 'tools/demo/bin/helper.sh');
  atomicWrite(file, '#!/bin/bash\necho nope\n');
  assert.ok(lintScripts(c.root).errors.some((error) => error.includes('nonportable executable')));
});


test('real process execution preserves literal arguments and propagates exit failure', () => {
  const literal = 'space λ; $(never-run) `also-literal`';
  const output = run(process.execPath, ['-e', 'process.stdout.write(process.argv[1])', literal]);
  assert.equal(output, literal);
  assert.throws(() => run(process.execPath, ['-e', 'process.exit(7)']), /failed \(7\)/);
});

test('known PRs converted to drafts leave the agenda and return when ready', (t) => {
  const c = fixture(t);
  scan(c, { now: NOW, exec: () => response([pr(12)]) });
  assert.equal(jsonFile(stored(c, 12)).is_draft, false);
  const draft = { ...pr(12), isDraft: true, title: 'Draft after initial review', headRefOid: 'd'.repeat(40), commits: { nodes: [{ commit: { oid: 'd'.repeat(40), committedDate: NOW } }] } };
  draft.reviews = connection([], true);
  const converted = scan(c, { now: NOW, exec: () => response([draft, { ...pr(13), isDraft: true }]) });
  assert.equal(converted.complete, true);
  const saved = jsonFile(stored(c, 12));
  assert.equal(saved.is_draft, true); assert.equal(saved.head_sha, 'd'.repeat(40));
  assert.notEqual(saved.status, 'CLOSED_OR_MERGED');
  assert.equal(fs.existsSync(stored(c, 13)), false);
  const agenda = fs.readFileSync(path.join(c.home, 'INBOX.md'), 'utf8');
  assert.ok(!agenda.includes('**api #12**'));
  assert.match(agenda, /1 tracked draft\(s\) hidden/);
  assert.match(fs.readFileSync(path.join(c.home, 'ledger.md'), 'utf8'), /draft; excluded from agenda/);
  scan(c, { now: NOW, exec: () => response([pr(12)]) });
  assert.equal(jsonFile(stored(c, 12)).is_draft, false);
  assert.match(fs.readFileSync(path.join(c.home, 'INBOX.md'), 'utf8'), /\*\*api #12\*\*/);
  scan(c, { now: NOW, exec: () => response([]) });
  assert.equal(jsonFile(stored(c, 12)).status, 'CLOSED_OR_MERGED');
});

test('repository aliases cannot collide on case-insensitive filesystems', (t) => {
  const c = fixture(t);
  const raw = jsonFile(c.file);
  saveJson(c.file, { ...raw, repos: { api: 'first-service', API: 'second-service' } });
  assert.throws(() => readConfig({ config: c.file }), /unique ignoring case/);
  saveJson(c.file, { ...raw, repos: { api: 'first-service', worker: 'second-service' } });
  assert.deepEqual(Object.keys(readConfig({ config: c.file }).repos), ['api', 'worker']);
});

test('absolute CLI invocation discovers consumer configuration and accepts explicit scope elsewhere', (t) => {
  const c = fixture(t);
  const cli = path.resolve('tools/craft-review-inbox/bin/inbox.mjs');
  const discovered = spawnSync(process.execPath, [cli, 'config'], { cwd: c.root, encoding: 'utf8' });
  assert.equal(discovered.status, 0, discovered.stderr);
  assert.equal(JSON.parse(discovered.stdout).root, c.root);
  const explicit = spawnSync(process.execPath, [cli, 'config', '--config', c.file], { cwd: path.dirname(cli), encoding: 'utf8' });
  assert.equal(explicit.status, 0, explicit.stderr);
  assert.equal(JSON.parse(explicit.stdout).root, c.root);
});
