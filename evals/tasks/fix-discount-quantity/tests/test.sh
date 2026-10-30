#!/bin/bash
# Grades one trial: hidden behavior, repository discipline, and what Craft recorded in /app.
set -uo pipefail
out=/logs/verifier
mkdir -p "$out"
cd /app || exit 1
# The artifact belongs to the agent user; let root read its history.
export GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=safe.directory GIT_CONFIG_VALUE_0=/app

check() { if "$@" >>"$out/checks.log" 2>&1; then echo 1; else echo 0; fi; }
correct=$(check node --test /tests/hidden/discount.test.js)
conventions=$(check node --test /tests/hidden/rounding.test.js)
repo_tests=$(check npm test --silent)
tests_untouched=$(check sha256sum -c /tests/public.sha256)
# Only source files may change; Craft's own state is ignored by the fixture's .gitignore.
scope_ok=$(check test -z "$(git status --porcelain --untracked-files=all -- . ':!src/')")

/opt/craft-eval/craft-rewards.sh /app /tests/expected.json "$out" \
  | jq --argjson correct "$correct" --argjson conventions "$conventions" --argjson repo_tests "$repo_tests" \
    --argjson tests_untouched "$tests_untouched" --argjson scope_ok "$scope_ok" \
    '{reward: $correct, $correct, $conventions, $repo_tests, $tests_untouched, $scope_ok} + .' >"$out/reward.json"
cat "$out/reward.json"
