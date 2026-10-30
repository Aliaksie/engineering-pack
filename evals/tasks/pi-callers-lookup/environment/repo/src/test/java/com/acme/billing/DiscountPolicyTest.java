package com.acme.billing;

import static org.junit.jupiter.api.Assertions.assertEquals;

import org.junit.jupiter.api.Test;

class DiscountPolicyTest {
    private final DiscountPolicy policy = new DiscountPolicy();

    @Test
    void appliesTenPercent() {
        assertEquals(900, policy.applyDiscount(1000, new Customer("c1", "silver", false), 10));
    }

    @Test
    void reversesADiscount() {
        assertEquals(1000, policy.reverseDiscount(900, 10));
    }
}
