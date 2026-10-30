#!/bin/bash
# Reference answer: every line that spells the setting, from a literal search.
set -euo pipefail
cd /app
grep -rn --exclude-dir=.git --exclude-dir=.craft LEGACY_TAX_RATE . | sed 's|^\./||' | cut -d: -f1,2 | sort >ANSWER.md
