#!/bin/bash
# Reference plan: grounded in the files and functions that compute and apply the discount.
set -euo pipefail
cat >/app/PLAN.md <<'PLAN'
# Plan: per-storefront promo discount cap

1. `src/cart.js` `discount(items, percent)`: fix the per-unit base first (it ignores
   `quantity`), then accept an optional `capCents` and return `Math.min(raw, capCents)`.
2. `src/order.js` `orderTotal`: add a `discountCapCents` option and pass it to `discount`.
   Tax stays computed on the capped discount.
3. Verify with new cases in `test/order.test.js`: a cart below the cap, one above it, a
   zero cap and multi-quantity lines; run `npm test`.
PLAN
