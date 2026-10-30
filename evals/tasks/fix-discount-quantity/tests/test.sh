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

# Craft's local telemetry; absent means the runtime never ran (or Craft is not installed).
events=/app/.craft/state/telemetry/events.jsonl
if [ -f "$events" ]; then cp "$events" "$out/craft-events.jsonl"; else : >"$out/craft-events.jsonl"; fi
count() { jq -s --arg name "$1" '[.[] | select(.name == $name)] | length' "$out/craft-events.jsonl"; }
jq -r 'select(.name == "guidance.selected") | .attributes.skill' "$out/craft-events.jsonl" | sort | uniq -c >"$out/craft-skills.txt"
expected=$(jq -s '[.[] | select(.name == "guidance.selected") | .attributes.skill] as $selected
  | ($expected[0].expectedSkills - $selected | length) == 0 | if . then 1 else 0 end' \
  --slurpfile expected /tests/expected.json "$out/craft-events.jsonl")
task_bound=$(check test -n "$(find /app/.craft/state -name task.json -print -quit 2>/dev/null)")

jq -n --argjson correct "$correct" --argjson conventions "$conventions" --argjson repo_tests "$repo_tests" \
  --argjson tests_untouched "$tests_untouched" --argjson scope_ok "$scope_ok" \
  --argjson craft_events "$(jq -s length "$out/craft-events.jsonl")" --argjson craft_hooks "$(count host.hook)" \
  --argjson craft_routing "$(count routing.completed)" --argjson craft_guidance "$(count guidance.selected)" \
  --argjson craft_lookups "$(count discovery.completed)" --argjson craft_checkpoints "$(count checkpoint.saved)" \
  --argjson craft_task_bound "$task_bound" --argjson expected_skill_selected "$expected" \
  '{reward: $correct, $correct, $conventions, $repo_tests, $tests_untouched, $scope_ok, $craft_events, $craft_hooks,
    $craft_routing, $craft_guidance, $craft_lookups, $craft_checkpoints, $craft_task_bound, $expected_skill_selected}' \
  >"$out/reward.json"
cat "$out/reward.json"
