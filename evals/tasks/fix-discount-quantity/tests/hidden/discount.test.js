import test from 'node:test';
import assert from 'node:assert/strict';
import { orderTotal } from '/app/src/order.js';

test('discounts every unit of a multi-quantity line', () => {
  const order = orderTotal([{ unitCents: 1000, quantity: 3 }], { discountPercent: 10 });
  assert.equal(order.discount, 300);
  assert.equal(order.total, 2700);
});

test('discounts mixed carts before tax and never discounts shipping', () => {
  const order = orderTotal([{ unitCents: 1250, quantity: 2 }, { unitCents: 499, quantity: 1 }],
    { discountPercent: 20, taxPercent: 10, shippingCents: 700 });
  assert.deepEqual(order, { subtotal: 2999, discount: 600, shipping: 700, tax: 310, total: 3409 });
});

test('keeps single-item, zero and full discounts', () => {
  assert.equal(orderTotal([{ unitCents: 2000, quantity: 1 }], { discountPercent: 10 }).discount, 200);
  assert.equal(orderTotal([{ unitCents: 999, quantity: 4 }]).discount, 0);
  assert.equal(orderTotal([{ unitCents: 999, quantity: 4 }], { discountPercent: 100 }).total, 0);
});

test('still rejects invalid input', () => {
  assert.throws(() => orderTotal([{ unitCents: 100, quantity: 0 }]), RangeError);
  assert.throws(() => orderTotal([{ unitCents: 100, quantity: 1 }], { discountPercent: 120 }), RangeError);
});
