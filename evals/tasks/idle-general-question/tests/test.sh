#!/bin/bash
# Grades one trial: the first number in the answer, nothing else changed, and what Craft
# recorded in /app.
set -uo pipefail
out=/logs/verifier
mkdir -p "$out"
cd /app || exit 1
export GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=safe.directory GIT_CONFIG_VALUE_0=/app

answered=$(grep -oE '[0-9]+' ANSWER.md 2>/dev/null | head -n 1)
printf '%s\n' "$answered" >"$out/answered.txt"
correct=$(if [ "$answered" = "429" ]; then echo 1; else echo 0; fi)
scope_ok=$(if [ -z "$(git status --porcelain --untracked-files=all -- . ':!ANSWER.md')" ]; then echo 1; else echo 0; fi)

/opt/craft-eval/craft-rewards.sh /app /tests/expected.json "$out" \
  | jq --argjson correct "$correct" --argjson scope_ok "$scope_ok" \
    '{reward: $correct, $correct, $scope_ok} + .' \
  >"$out/reward.json"
cat "$out/reward.json"
