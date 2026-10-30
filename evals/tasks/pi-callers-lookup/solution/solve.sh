#!/bin/bash
# Reference answer: the four direct call sites, not the comment, log text or reverseDiscount.
set -euo pipefail
cat >/app/ANSWER.md <<'ANSWER'
src/main/java/com/acme/billing/Checkout.java#finish
src/main/java/com/acme/billing/Quote.java#preview
src/main/java/com/acme/billing/Subscription.java#renew
src/test/java/com/acme/billing/DiscountPolicyTest.java#appliesTenPercent
ANSWER
