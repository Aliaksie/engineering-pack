package com.acme.billing.legacy;

import java.util.logging.Logger;

/** Kept for the reporting export; prices are computed by the current checkout. */
public class OldCheckout {
    private static final Logger LOG = Logger.getLogger(OldCheckout.class.getName());

    // Exports before 2.0 applied LEGACY_TAX_RATE on top of the discounted price.
    public long total(long cents) {
        LOG.fine("applyDiscount skipped: legacy orders are already discounted");
        return cents;
    }
}
