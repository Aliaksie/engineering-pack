#!/bin/bash
# Grades one trial: the listed mentions against every line that spells the setting, and what
# Craft recorded.
set -uo pipefail
out=/logs/verifier
mkdir -p "$out"
cd /app || exit 1
export GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=safe.directory GIT_CONFIG_VALUE_0=/app

# `path:line` entries anywhere in the answer, ignoring list markers and code quotes.
grep -oE '[A-Za-z0-9_./-]+\.[A-Za-z]+:[0-9]+' ANSWER.md 2>/dev/null | sed 's|^\./||' | sort -u >"$out/answered.txt"
sort -u /tests/mentions.txt >"$out/expected.txt"
found=$(comm -12 "$out/expected.txt" "$out/answered.txt" | wc -l)
wrong=$(comm -13 "$out/expected.txt" "$out/answered.txt" | wc -l)
expected=$(wc -l <"$out/expected.txt")
scope_ok=$(if [ -z "$(git status --porcelain --untracked-files=all -- . ':!ANSWER.md')" ]; then echo 1; else echo 0; fi)

/opt/craft-eval/craft-rewards.sh /app /tests/expected.json "$out" \
  | jq --argjson found "$found" --argjson wrong "$wrong" --argjson expected "$expected" --argjson scope_ok "$scope_ok" \
    '{reward: (if $found == $expected and $wrong == 0 then 1 else 0 end),
      mentions_found: $found, mentions_expected: $expected, mentions_wrong: $wrong, $scope_ok} + .' \
  >"$out/reward.json"
cat "$out/reward.json"
