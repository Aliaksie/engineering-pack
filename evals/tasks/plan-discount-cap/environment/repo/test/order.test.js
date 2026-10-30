import test from 'node:test';
import assert from 'node:assert/strict';
import { orderTotal } from '../src/order.js';
import { format } from '../src/money.js';

test('totals a single item with tax and shipping', () => {
  const order = orderTotal([{ unitCents: 2500, quantity: 1 }], { taxPercent: 8, shippingCents: 500 });
  assert.deepEqual(order, { subtotal: 2500, discount: 0, shipping: 500, tax: 240, total: 3240 });
});

test('discounts a single item before tax', () => {
  const order = orderTotal([{ unitCents: 2000, quantity: 1 }], { discountPercent: 10, taxPercent: 5 });
  assert.equal(order.discount, 200);
  assert.equal(order.total, 1890);
});

test('formats cents', () => {
  assert.equal(format(1890), '$18.90');
  assert.equal(format(-5), '-$0.05');
});
