package com.acme.billing;

public record Customer(String id, String tier, boolean employee) {
    public boolean loyal() {
        return "gold".equals(tier) || "platinum".equals(tier);
    }
}
