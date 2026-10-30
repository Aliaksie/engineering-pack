package com.acme.billing;

public class Subscription {
    private final DiscountPolicy policy;
    private final long monthlyCents;

    public Subscription(DiscountPolicy policy, long monthlyCents) {
        this.policy = policy;
        this.monthlyCents = monthlyCents;
    }

    public long renew(Customer customer) {
        int loyalty = customer.loyal() ? 15 : 0;
        return policy.applyDiscount(monthlyCents, customer, loyalty);
    }

    public long monthly() {
        return monthlyCents;
    }
}
