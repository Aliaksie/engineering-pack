#!/bin/bash
# Grades one trial: a plan grounded in the code that computes the discount, no implementation,
# and what Craft recorded (including whether it kept a work plan).
set -uo pipefail
out=/logs/verifier
mkdir -p "$out"
cd /app || exit 1
export GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=safe.directory GIT_CONFIG_VALUE_0=/app

check() { if "$@" >>"$out/checks.log" 2>&1; then echo 1; else echo 0; fi; }
plan_written=$(check test -s PLAN.md)
cites_code=$(check bash -c 'grep -q "src/cart.js" PLAN.md && grep -q "src/order.js" PLAN.md')
cites_functions=$(check bash -c 'grep -qE "\bdiscount\b" PLAN.md && grep -q "orderTotal" PLAN.md')
has_verification=$(check grep -qE "test/order\.test\.js|npm test|node --test" PLAN.md)
# Informative: the planner noticed the existing quantity defect the cap would sit on.
noticed_defect=$(check grep -qiE "quantity" PLAN.md)
# Cited source paths that do not exist and are not proposed as new files.
missing_paths=$(grep -oE '\bsrc/[A-Za-z0-9_./-]+\.js\b' PLAN.md 2>/dev/null | sort -u | while read -r file; do [ -f "$file" ] || echo "$file"; done | wc -l)
source_untouched=$(check test -z "$(git status --porcelain --untracked-files=all -- . ':!PLAN.md')")
tests_untouched=$(check sha256sum -c /tests/public.sha256)

/opt/craft-eval/craft-rewards.sh /app /tests/expected.json "$out" \
  | jq --argjson plan_written "$plan_written" --argjson cites_code "$cites_code" --argjson cites_functions "$cites_functions" \
    --argjson has_verification "$has_verification" --argjson noticed_defect "$noticed_defect" --argjson missing_paths "$missing_paths" \
    --argjson source_untouched "$source_untouched" --argjson tests_untouched "$tests_untouched" \
    '{reward: ([$plan_written, $cites_code, $cites_functions, $has_verification, $source_untouched] | min),
      $plan_written, $cites_code, $cites_functions, $has_verification, $noticed_defect, $missing_paths,
      $source_untouched, $tests_untouched} + .' >"$out/reward.json"
cat "$out/reward.json"
