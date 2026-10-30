#!/bin/sh
# Nightly export for the reporting team.
set -eu
export LEGACY_TAX_RATE="${LEGACY_TAX_RATE:-0}"
exec java -cp target/classes com.acme.billing.legacy.OldCheckout "$@"
