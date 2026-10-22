import test from 'node:test';
import assert from 'node:assert/strict';
import { receiptStore } from './src/receipt.mjs';

test('retry uses canonical event identity', () => {
  const store = receiptStore();
  assert.deepEqual(store.apply(' Event-7 '), { applied: true, total: 1 });
  assert.deepEqual(store.apply('event-7'), { applied: false, total: 1 });
  assert.deepEqual(store.apply('EVENT-7'), { applied: false, total: 1 });
  assert.deepEqual(store.apply('event-8'), { applied: true, total: 2 });
});

test('invalid key is rejected without consuming a receipt', () => {
  const store = receiptStore();
  for (const key of ['', '  ', null, 42]) assert.throws(() => store.apply(key), TypeError);
  assert.deepEqual(store.apply('event-7'), { applied: true, total: 1 });
});
