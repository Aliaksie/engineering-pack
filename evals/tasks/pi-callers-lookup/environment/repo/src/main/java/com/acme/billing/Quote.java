package com.acme.billing;

public class Quote {
    private final DiscountPolicy policy = new DiscountPolicy();

    public String preview(long cents, Customer customer, int percent) {
        long price = policy.applyDiscount(cents, customer, percent);
        return String.format("%d.%02d", price / 100, price % 100);
    }
}
