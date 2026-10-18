import test from 'node:test';
import assert from 'node:assert/strict';
import { applyEvent } from './src/apply-event.mjs';
import { orderState } from './src/order-state.mjs';

test('retry does not apply a previously accepted event twice', () => {
  const state = orderState(), event = { account: 'a', id: '1', delta: 4 };
  assert.deepEqual(applyEvent(state, event), { account: 'a', id: '1', total: 4, duplicate: false });
  assert.deepEqual(applyEvent(state, event), { account: 'a', id: '1', total: 4, duplicate: true });
  assert.equal(state.applied.size, 1);
});
test('different accounts can use the same event identifier', () => {
  const state = orderState();
  applyEvent(state, { account: 'a', id: '1', delta: 4 });
  assert.equal(applyEvent(state, { account: 'b', id: '1', delta: 7 }).total, 7);
});
test('duplicate receipt reports the current total after later events', () => {
  const state = orderState(), first = { account: 'a', id: '1', delta: 4 };
  applyEvent(state, first);
  applyEvent(state, { account: 'a', id: '2', delta: 3 });
  assert.deepEqual(applyEvent(state, first), { account: 'a', id: '1', total: 7, duplicate: true });
});
test('invalid input cannot be accepted or alter the totals', () => {
  const state = orderState();
  assert.throws(() => applyEvent(state, { account: 'a', id: '1', delta: 1.2 }), /invalid event/);
  assert.equal(state.applied.size, 0);
  assert.equal(state.totals.size, 0);
});
