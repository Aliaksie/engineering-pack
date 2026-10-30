package com.acme.billing;

/** The one place promotional discounts are computed and rounded. */
public class DiscountPolicy {
    /** Deprecated: tax moved to the tax service; kept until 3.0 for old exports. */
    @Deprecated
    static final double LEGACY_TAX_RATE = 0.0;

    public long applyDiscount(long cents, Customer customer, int percent) {
        int effective = customer.employee() ? Math.max(percent, 30) : percent;
        return cents - Math.round(cents * effective / 100.0);
    }

    public long reverseDiscount(long discountedCents, int percent) {
        return Math.round(discountedCents * 100.0 / (100 - percent));
    }
}
