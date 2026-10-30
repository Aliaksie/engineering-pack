package com.acme.billing;

public class Refunds {
    private final DiscountPolicy policy = new DiscountPolicy();

    // Do not call applyDiscount here: a refund restores the price the customer paid.
    public long refund(long paidCents, int percent) {
        return policy.reverseDiscount(paidCents, percent);
    }
}
