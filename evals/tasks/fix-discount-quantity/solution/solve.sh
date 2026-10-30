#!/bin/bash
# Reference fix: discount the merchandise subtotal, not one unit per line.
set -euo pipefail
cd /app
sed -i 's|return items.reduce((sum, item) => sum + percentOf(item.unitCents, percent), 0);|return percentOf(subtotal(items), percent);|' src/cart.js
