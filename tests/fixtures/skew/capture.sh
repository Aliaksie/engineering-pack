#!/bin/sh
# Capture project, user file and state written by one released craft binary.
# usage: skew-capture.sh <craft-binary> <new-out-dir>
set -eu
bin=$1; out=$2
[ -e "$out" ] && { echo "$out exists" >&2; exit 1; }
mkdir -p "$out/project" "$out/home/.config/craft"
out=$(cd "$out" && pwd -P); p=$out/project
export HOME=$out/home GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 GIT_AUTHOR_DATE=2026-11-01T00:00:00Z GIT_COMMITTER_DATE=2026-11-01T00:00:00Z
unset CRAFT_ORGANIZATION_CONFIG CRAFT_PACKAGE_ROOT || true
cat > "$HOME/.config/craft/preferences.json" <<'J'
{"formatVersion": 1, "routing": {"sessionSummary": true, "debtMarkers": true}, "safety": {"enabled": true}}
J
cd "$p"
git init -q -b main && git config user.email fixture@example.test && git config user.name Fixture
mkdir -p .craft src
cat > .craft/project.json <<'J'
{"formatVersion": 1,
 "routing": {"debtMarkers": true},
 "modules": [{"name": "core", "path": "src", "languages": ["rust"], "dependsOn": [],
   "checks": [{"id": "core-ok", "command": "true", "args": [], "stage": "commit"}]}],
 "standards": [{"id": "commits", "anchor": "conventional-commits"},
   {"id": "changelog-line", "summary": "Observable change adds a CHANGELOG line.", "checklist": ["CHANGELOG has a line"], "scope": {"paths": ["src"]}}],
 "safety": {"enabled": true}}
J
printf 'pub fn a() {}\n' > src/lib.rs
git add -A && git commit -q -m "chore: start"
S=skew-session-1
hook() { # host lifecycle package handler scope payload
  printf '%s' "$6" | "$bin" hook "$1" "$2" --package "$3" --handler "$4" --scope "$5" > /dev/null 2>&1 || true
}
both() { # lifecycle payload
  hook claude "$1" craft-delivery deliver any "$2"; hook claude "$1" craft-engineering observe any "$2"
}
base="\"session_id\":\"$S\",\"cwd\":\"$p\""
both session-start "{$base,\"source\":\"startup\"}"
both before-prompt "{$base,\"prompt\":\"implement a second function in src/lib.rs\"}"
both before-tool "{$base,\"tool_name\":\"Edit\",\"tool_input\":{\"file_path\":\"$p/src/lib.rs\"}}"
printf 'pub fn a() {}\n// TODO: finish b\npub fn b() {}\n' > src/lib.rs
both after-tool "{$base,\"tool_name\":\"Edit\",\"tool_input\":{\"file_path\":\"$p/src/lib.rs\"},\"tool_response\":{}}"
both before-tool "{$base,\"tool_name\":\"Bash\",\"tool_input\":{\"command\":\"cargo test\"}}"
both after-tool "{$base,\"tool_name\":\"Bash\",\"tool_input\":{\"command\":\"cargo test\"},\"tool_response\":{\"stdout\":\"running 2 tests\\ntest result: ok. 2 passed; 0 failed; 0 ignored\\n\",\"stderr\":\"\"}}"
"$bin" checkpoint-save --root "$p" --session "$S" --summary "second function" >/dev/null 2>&1 || true
"$bin" check --root "$p" --stage commit --session "$S" >/dev/null 2>&1 || true
both before-compact "{$base,\"trigger\":\"auto\"}"
both after-compact "{$base,\"source\":\"compact\"}"
both turn-end "{$base,\"stop_hook_active\":false}"
hook claude agent-start craft-delivery deliver any "{$base,\"agent_type\":\"general\"}"
find "$p/.craft" -type f | sed "s#$p/##" | sort
