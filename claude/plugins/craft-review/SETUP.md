# First-use setup and session visibility

On first use of Craft Capabilities in a conversation, give one brief setup notice
alongside the requested work. Reuse it across pack packages; repeat only when setup
changes or the user asks. This guidance works when a skill loads the package
instructions; installation alone cannot guarantee a welcome message. Never rely
on an unverified hook to deliver the notice explaining how to activate hooks.

Use the compact export facts on this page for hook support, native controls,
usage observation and the portable status entry point. Reserve the full `SETUP.json`
feature matrix for an explicit setup or capability inspection. Establish the actual host surface
and version before giving UI commands. An installed binary does not establish which
app runs this session. If the surface is unclear, ask once while continuing work.

The notice should tell the user what is available, what remains unverified, and
the next relevant action. Use the adapter's documented controls only on their
supported surface. When a command or menu is absent, say that activation remains
unverified and consult that surface's documentation; do not invent a menu or
retry unsupported commands. Guidance-only exports must say that automatic hooks
are not registered. Package installation, project trust, hook registration and
hook trust are separate facts. Never enable or trust hooks on the user's behalf.

Users can ask: **"Check Craft Capabilities setup, hook activity and session usage."**
When asked, resolve the actual installed package root from the loaded skill's
resource links and, where shell execution is available, run `sessionAdvice.entry` with its
`args` from the export facts and the target project:

```sh
"<package-root>/bin/craft" statusline <host> --package <package> --root "<project-root>" --json
```

Replace the placeholders with verified paths and the listed host and package; on Windows
run `bin\craft.cmd`. This is an explicit status read,
works without hooks, writes local operational telemetry by default, and does not
configure the host UI. If execution is unavailable, explain the manual fallback
and continue with guidance. Do not run commands merely to show the setup notice.

With default local telemetry enabled, compare events before and after an ordinary
edit/tool action in the target project's runtime `telemetry.jsonl`; the bundled CLI's
`settings --root <project-root>` reports its private state directory. Current exports use
`.craft/state/<worktree-id>/shared/telemetry/events.jsonl`; older exports may use
`shared/telemetry.jsonl`, Git-private or flat workspace-local storage. Stop older writers before upgrading; see the runtime storage
migration documentation.
Fresh `host.hook` events establish observed hook invocation; explicit CLI events
and direct wrapper calls do not prove automatic host dispatch. Missing events
alone do not distinguish discovery, trust, disabled telemetry or storage failures.
Report unsupported, unverified and observed behavior separately.

Worker/review/compaction recommendations do not install native execution transports.
Trusted application controller callbacks require a separately connected host adapter;
settings and hook activity alone do not establish that connection. Keep main work usable
when these controls are unavailable and report actual adapter observations separately.
Each export's `SETUP.json` includes per-feature support and native binding requirements.
Inspect these for the actual host; copied runtime files are not connected native controls.

For bounded retained evidence, the bundled CLI's `telemetry-report --root <project-root>`
reports feature observations, package/input attribution, reported drops and retention limits.
Use `--session <id>` only with a known real session identity. Missing observations do not
establish disabled hooks, and rotation/loss prevents complete-history claims.
For an explicit audit/evaluation milestone, `telemetry-capture --root <project-root> --session <id>`
retains the available report in bounded private storage and returns a capture ID. Read it with
`telemetry-report --capture <id>`; remove it only through an explicit `telemetry-forget --capture <id>`.
Capacity refuses new captures rather than evicting existing evidence. Captures retain the
report's unknown-history limits and do not replace the separate evaluation artifacts.

Offer native usage inspection or display controls when the adapter and actual
surface support them. Distinguish cumulative session tokens, current context
occupancy and selected-guidance estimates. Keep unavailable counts unknown; a
working hook does not establish token visibility. Never invent counts or promise
a persistent display where the host exposes none. Report the actual capability
gap and manual next step without claiming automatic setup succeeded.

## Export facts

```json
{
  "adapter": "claude",
  "hooks": "native-before-tool",
  "sessionAdvice": {
    "entry": "bin/craft",
    "args": [
      "statusline",
      "claude",
      "--package",
      "craft-review"
    ],
    "observation": "statusline-current-usage",
    "display": "optional-native-statusline",
    "commands": {
      "compact": "/compact",
      "start-new-session": "/clear"
    },
    "activation": "explicit-only; no statusline settings or hooks changed"
  },
  "controls": {
    "surface": "Use the actual host surface and version documentation",
    "commands": {}
  }
}
```
