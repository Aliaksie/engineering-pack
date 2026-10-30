import { discount, subtotal } from './cart.js';
import { percentOf } from './money.js';

// Tax applies to discounted merchandise plus shipping.
export function orderTotal(items, { discountPercent = 0, taxPercent = 0, shippingCents = 0 } = {}) {
  const merchandise = subtotal(items);
  const off = discount(items, discountPercent);
  const taxable = merchandise - off + shippingCents;
  const tax = percentOf(taxable, taxPercent);
  return { subtotal: merchandise, discount: off, shipping: shippingCents, tax, total: taxable + tax };
}
