#!/bin/bash
# Grades a cited answer against a probe's ground truth. Usage:
#   answer-grade.sh <repository> <truth.json> <output directory>
# truth.json: {"format": "path"|"symbol", "match": "all"|"any", "expected": [...], "allowed": [...]}
# A path citation counts when it names a file of the repository or contains a slash; a symbol
# citation is `<path>#<name>`, compared by its last dotted segment (`Class.method` → `method`).
# Prints one JSON object of numeric reward values.
set -uo pipefail
repo=$1 truth=$2 out=$3
answer="$repo/ANSWER.md"
format=$(jq -r '.format' "$truth")

normalize() {
  sed -E 's#^\./##; s#^/app/##; s#[.]+$##' | if [ "$format" = symbol ]; then
    sed -E 's@#(.*\.)?([^.]+)$@#\2@'
  else cat; fi
}

: >"$out/answered.txt"
if [ -f "$answer" ]; then
  if [ "$format" = symbol ]; then
    grep -oE '[A-Za-z0-9_./@$-]+\.[A-Za-z0-9]+#[A-Za-z0-9_$.]+' "$answer"
  else
    grep -oE '[A-Za-z0-9_./@$-]+\.[A-Za-z0-9]+' "$answer"
  fi | normalize | while read -r cited; do
    file=${cited%%#*}
    # Prose such as "e.g." or "chi.Router" is not a citation unless it names a real file.
    if [ -f "$repo/$file" ] || [[ "$file" == */* ]]; then echo "$cited"; fi
  done | sort -u >"$out/answered.txt"
fi
jq -r '.expected[]' "$truth" | normalize | sort -u >"$out/expected.txt"
jq -r '(.allowed // [])[]' "$truth" | normalize | sort -u >"$out/allowed.txt"

found=$(comm -12 "$out/expected.txt" "$out/answered.txt" | wc -l | tr -d ' ')
wrong=$(sort -u "$out/expected.txt" "$out/allowed.txt" | comm -13 - "$out/answered.txt" | wc -l | tr -d ' ')
expected=$(wc -l <"$out/expected.txt" | tr -d ' ')
invented=$(cut -d'#' -f1 "$out/answered.txt" | sort -u | while read -r file; do [ -f "$repo/$file" ] || echo "$file"; done | wc -l | tr -d ' ')

jq -n --arg match "$(jq -r '.match // "all"' "$truth")" \
  --argjson found "$found" --argjson wrong "$wrong" --argjson expected "$expected" --argjson invented "$invented" \
  '{reward: (if $wrong == 0 and $invented == 0 and (if $match == "any" then $found >= 1 else $found == $expected end) then 1 else 0 end),
    cited_found: $found, cited_expected: $expected, cited_wrong: $wrong, invented_paths: $invented}'
