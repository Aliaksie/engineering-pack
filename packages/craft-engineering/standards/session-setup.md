# First-use setup and session visibility

On first use of Engineering Pack in a conversation, give one brief setup notice
alongside the requested work. Reuse it across pack packages; repeat only when setup
changes or the user asks. This guidance works when a skill loads the package
instructions; installation alone cannot guarantee a welcome message. Never rely
on an unverified hook to deliver the notice explaining how to activate hooks.

Read the colocated `SETUP.json` for this export's hook support, native controls,
usage observation and portable status entry point. Establish the actual host surface
and version before giving UI commands. An installed binary does not establish which
app runs this session. If the surface is unclear, ask once while continuing work.

The notice should tell the user what is available, what remains unverified, and
the next relevant action. Use the adapter's documented controls only on their
supported surface. When a command or menu is absent, say that activation remains
unverified and consult that surface's documentation; do not invent a menu or
retry unsupported commands. Guidance-only exports must say that automatic hooks
are not registered. Package installation, project trust, hook registration and
hook trust are separate facts. Never enable or trust hooks on the user's behalf.

Users can ask: **"Check Engineering Pack setup, hook activity and session usage."**
When asked, resolve the actual installed package root from the loaded skill's
resource links and, where shell execution and Node 20+ are available, run:

```sh
node "<package-root>/support/session.mjs" --root "<project-root>" --json
```

Replace both placeholders with verified paths. This is an explicit status read,
works without hooks, writes local operational telemetry by default, and does not
configure the host UI. If execution is unavailable, explain the manual fallback
and continue with guidance. Do not run commands merely to show the setup notice.

With default local telemetry enabled, compare events before and after an ordinary
edit/tool action in the target project's runtime `telemetry.jsonl`; the bundled CLI's
`settings --root <project-root>` reports its private state directory. Older exports may
use `.capabilities/state/telemetry.jsonl`.
Fresh `host.hook` events establish observed hook invocation; explicit CLI events
and direct wrapper calls do not prove automatic host dispatch. Missing events
alone do not distinguish discovery, trust, disabled telemetry or storage failures.
Report unsupported, unverified and observed behavior separately.

Offer native usage inspection or display controls when the adapter and actual
surface support them. Distinguish cumulative session tokens, current context
occupancy and selected-guidance estimates. Keep unavailable counts unknown; a
working hook does not establish token visibility. Never invent counts or promise
a persistent display where the host exposes none. Report the actual capability
gap and manual next step without claiming automatic setup succeeded.
