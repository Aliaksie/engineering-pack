import test from 'node:test';
import assert from 'node:assert/strict';
import { orderDelayed, orderShipped } from '../src/messages.js';
import { delayFor, withRetry } from '../src/retry.js';

test('shipped message links tracking', () => {
  assert.match(orderShipped({ id: 'A1', trackingUrl: 'https://t.example/A1' }), /https:\/\/t\.example\/A1/);
});

test('delay message uses singular for one day', () => {
  assert.match(orderDelayed({ id: 'A1' }, 1), /1 day late/);
});

test('delay grows by the factor', () => {
  assert.deepEqual([0, 1, 2].map(retry => delayFor(retry)), [200, 400, 800]);
});

test('a delivery that succeeds is not retried', async () => {
  let calls = 0;
  assert.equal(await withRetry(async () => { calls++; return 'ok'; }, { sleep: async () => {} }), 'ok');
  assert.equal(calls, 1);
});
