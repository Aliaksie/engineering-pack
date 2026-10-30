import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import test from 'node:test';
import { deliverHook } from './native-hook.mjs';

const corpus = JSON.parse(fs.readFileSync(new URL('./fixtures/decisions/prompt-routing.json', import.meta.url), 'utf8'));

// Replays each prompt through the prompt-submit hook as Claude runs it. A changed decision fails
// until `current` is updated; the distance to `expected` is reported, not enforced.
test('prompt routing replay', t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-decision-replay-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const gaps = [];
  for (const { id, prompt, expected, current } of corpus.cases) {
    // A separate session per case: an identical hint is suppressed within one session.
    const data = deliverHook(root, 'before-prompt', { prompt, session: id, host: 'claude' }).context?.data;
    const observed = { intent: data?.intent ?? null, skills: data?.skills ?? [] };
    assert.deepEqual(observed, current, `${id}: routing changed; update current deliberately`);
    if (observed.intent !== expected.intent) {
      const kind = observed.intent === null ? 'missed' : expected.intent === null ? 'unwanted' : 'wrong';
      gaps.push(`${kind} ${id}: expected ${expected.intent}, got ${observed.intent}`);
    }
  }
  t.diagnostic(`${corpus.cases.length - gaps.length}/${corpus.cases.length} prompts routed as expected`);
  for (const gap of gaps) t.diagnostic(gap);
});
