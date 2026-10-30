package com.acme.billing;

/** The one place promotional discounts are computed and rounded. */
public class DiscountPolicy {
    public long applyDiscount(long cents, Customer customer, int percent) {
        int effective = customer.employee() ? Math.max(percent, 30) : percent;
        return cents - Math.round(cents * effective / 100.0);
    }

    public long reverseDiscount(long discountedCents, int percent) {
        return Math.round(discountedCents * 100.0 / (100 - percent));
    }
}
