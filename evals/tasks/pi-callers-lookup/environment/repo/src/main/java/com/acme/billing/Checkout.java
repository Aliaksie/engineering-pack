package com.acme.billing;

import java.util.List;

public class Checkout {
    private final DiscountPolicy policy = new DiscountPolicy();

    public long subtotal(List<Long> lines) {
        return lines.stream().mapToLong(Long::longValue).sum();
    }

    public long finish(List<Long> lines, Customer customer, int promoPercent) {
        long gross = subtotal(lines);
        return policy.applyDiscount(gross, customer, promoPercent);
    }
}
