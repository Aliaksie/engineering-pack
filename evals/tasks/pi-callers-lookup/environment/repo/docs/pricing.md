# Pricing

All promotional discounts go through `applyDiscount` so rounding stays in one place.
Refunds reverse a discount with `reverseDiscount`; they never call `applyDiscount` again.
