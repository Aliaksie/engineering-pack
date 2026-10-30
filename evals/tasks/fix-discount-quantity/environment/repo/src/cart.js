import { percentOf } from './money.js';

export function lineTotal(item) {
  if (!Number.isInteger(item.unitCents) || item.unitCents < 0) throw new RangeError('unitCents must be a non-negative integer');
  if (!Number.isInteger(item.quantity) || item.quantity < 1) throw new RangeError('quantity must be a positive integer');
  return item.unitCents * item.quantity;
}

export function subtotal(items) {
  return items.reduce((sum, item) => sum + lineTotal(item), 0);
}

// Discounts apply to merchandise only; shipping is never discounted.
export function discount(items, percent) {
  if (percent < 0 || percent > 100) throw new RangeError('discount must be between 0 and 100');
  return items.reduce((sum, item) => sum + percentOf(item.unitCents, percent), 0);
}
