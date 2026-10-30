#!/bin/bash
# Grades one trial: the caller list against the known call sites, and what Craft recorded.
set -uo pipefail
out=/logs/verifier
mkdir -p "$out"
cd /app || exit 1
export GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=safe.directory GIT_CONFIG_VALUE_0=/app

# `path#method` entries anywhere in the answer, ignoring list markers and code quotes.
grep -oE '[A-Za-z0-9_./-]+\.java#[A-Za-z0-9_]+' ANSWER.md 2>/dev/null | sed 's|^\./||' | sort -u >"$out/answered.txt"
sort -u /tests/callers.txt >"$out/expected.txt"
found=$(comm -12 "$out/expected.txt" "$out/answered.txt" | wc -l)
wrong=$(comm -13 "$out/expected.txt" "$out/answered.txt" | wc -l)
# A cited file that does not exist is an invention, not just a wrong caller.
invented=$(cut -d'#' -f1 "$out/answered.txt" | sort -u | while read -r file; do [ -f "$file" ] || echo "$file"; done | wc -l)
expected=$(wc -l <"$out/expected.txt")
# Only the answer may be added; Craft's own state is ignored by the fixture's .gitignore.
scope_ok=$(if [ -z "$(git status --porcelain --untracked-files=all -- . ':!ANSWER.md')" ]; then echo 1; else echo 0; fi)

/opt/craft-eval/craft-rewards.sh /app /tests/expected.json "$out" \
  | jq --argjson found "$found" --argjson wrong "$wrong" --argjson invented "$invented" --argjson expected "$expected" \
    --argjson scope_ok "$scope_ok" \
    '{reward: (if $found == $expected and $wrong == 0 then 1 else 0 end),
      callers_found: $found, callers_expected: $expected, callers_wrong: $wrong, invented_paths: $invented, $scope_ok} + .' \
  >"$out/reward.json"
cat "$out/reward.json"
