#!/bin/bash
# Grades one trial: the corrected text, nothing else changed, and what Craft recorded in /app.
set -uo pipefail
out=/logs/verifier
mkdir -p "$out"
cd /app || exit 1
export GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=safe.directory GIT_CONFIG_VALUE_0=/app

check() { if "$@" >>"$out/checks.log" 2>&1; then echo 1; else echo 0; fi; }
fixed() { grep -q 'was received and is being prepared' src/messages.js && ! git grep -qi 'recieved' -- .; }
correct=$(check fixed)
repo_tests=$(check npm test --silent)
tests_untouched=$(check sha256sum -c /tests/public.sha256)
# Only the message file may change; Craft's own state is ignored by the fixture's .gitignore.
scope_ok=$(check test -z "$(git status --porcelain --untracked-files=all -- . ':!src/messages.js')")

/opt/craft-eval/craft-rewards.sh /app /tests/expected.json "$out" \
  | jq --argjson correct "$correct" --argjson repo_tests "$repo_tests" --argjson tests_untouched "$tests_untouched" \
    --argjson scope_ok "$scope_ok" \
    '{reward: (if $correct == 1 and $scope_ok == 1 then 1 else 0 end), $correct, $repo_tests, $tests_untouched, $scope_ok} + .' \
  >"$out/reward.json"
cat "$out/reward.json"
