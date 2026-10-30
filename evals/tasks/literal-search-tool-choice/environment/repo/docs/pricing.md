# Pricing

All promotional discounts go through `applyDiscount` so rounding stays in one place.
Refunds reverse a discount with `reverseDiscount`; they never call `applyDiscount` again.

## Tax

Tax is computed by the tax service. `LEGACY_TAX_RATE` is deprecated and reads as zero.
