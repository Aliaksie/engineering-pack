import test from 'node:test';
import assert from 'node:assert/strict';
import { orderTotal } from '/app/src/order.js';

// The project rule (src/money.js): a percentage is applied once and rounded half up.
test('rounds the discount once over the merchandise subtotal', () => {
  const order = orderTotal([{ unitCents: 105, quantity: 1 }, { unitCents: 105, quantity: 1 }], { discountPercent: 10 });
  assert.equal(order.discount, 21);
});
