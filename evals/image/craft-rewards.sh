#!/bin/bash
# Shared by every task verifier: what Craft recorded in the collected repository, as reward
# values. Usage: craft-rewards.sh <repository> <expected.json> <output directory>
# Prints one JSON object; a value Craft never recorded is 0, so a silent runtime cannot pass.
set -uo pipefail
repo=$1 expected=$2 out=$3
state="$repo/.craft/state"
events="$out/craft-events.jsonl"

# Absent telemetry means the runtime never ran, or Craft is not installed in this arm.
if [ -f "$state/telemetry/events.jsonl" ]; then
  cat "$state/telemetry/events.jsonl.1" "$state/telemetry/events.jsonl" 2>/dev/null >"$events"
else
  : >"$events"
fi
jq -r 'select(.name == "guidance.selected") | .attributes.skill' "$events" | sort | uniq -c >"$out/craft-skills.txt"
count() { jq -s --arg name "$1" '[.[] | select(.name == $name)] | length' "$events"; }
present() { if [ -n "$(find "$state" -path "$1" -print -quit 2>/dev/null)" ]; then echo 1; else echo 0; fi; }

jq -s --slurpfile expected "$expected" \
  --argjson task_bound "$(present '*/task.json')" --argjson plans "$(present "$state/plans/*/*")" '
  def named($n): [.[] | select(.name == $n)];
  [named("guidance.selected")[].attributes.skill] as $selected
  | (named("host.hook") | map(.durationMs // 0) | sort) as $hooks
  | {
      craft_events: length,
      craft_hooks: ($hooks | length),
      craft_hook_ms_total: ($hooks | add // 0),
      craft_hook_ms_max: ($hooks | max // 0),
      craft_routing: (named("routing.completed") | length),
      craft_guidance: (named("guidance.selected") | length),
      craft_lookups: (named("discovery.completed") | length),
      craft_checkpoints: (named("checkpoint.saved") | length),
      craft_failures: ([.[] | select(.outcome == "failure")] | length),
      craft_task_bound: $task_bound,
      craft_plan_created: $plans,
      expected_skill_selected: (if (($expected[0].expectedSkills // []) - $selected | length) == 0 then 1 else 0 end)
    }' "$events"
