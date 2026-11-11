# Installation

Install from this repository's generated branches. You need Git and your agent host; no npm
dependencies, source checkout or local build.

## Channels

| Branch | Contents |
|---|---|
| `distribution` | Current stable release |
| `distribution-next` | Candidate built from `main` on every successful CI run |

Both channels bundle a native `craft` executable in each package; no Node is needed. The
commands below use `distribution`. To try the candidate, substitute `distribution-next` in a
disposable project. Guidance-only use needs no executable; local Git operations need Git.

Bundles ship executables for macOS 15+ on Apple Silicon, Linux x86-64 (glibc) and Windows
x86-64; the hook command or launcher picks the one for the current OS and CPU. Elsewhere
(macOS Intel, Linux ARM, musl) the read-only review guard denies shell commands, other hooks
continue, the package says once at session start that it is inactive, and an explicit command
fails with a clear error. Executables are not code-signed or notarized. Bundle checksums
detect damage; release assets also carry GitHub artifact attestations
([releasing](RELEASING.md)).

To roll back, reselect the complete previous bundle; never mix executables and adapters from
different bundles. A previous release is available as a GitHub Release asset or as an earlier
commit of `distribution` (for example `Aliaksie/engineering-pack#<commit>` in Claude Code or
`--ref <commit>` in Codex). Replacing a bundle keeps the consumer's private state.

## Claude Code

```sh
claude plugin marketplace add Aliaksie/engineering-pack#distribution
claude plugin install craft-engineering@craft-capabilities
```

In the target project invoke `/craft-engineering:foundations`. Install `craft-delivery`,
`craft-review` or `craft-stories` from the same marketplace when needed; workflow entry
points use the `workflow-` prefix, for example `/craft-stories:workflow-prepare-stories`.
Installation does not configure a task tracker.

Claude Code refuses an action when a plugin hook cannot start or return an answer. Each package
ships `bin/craft` with runtimes for Linux x64, macOS arm64 and Windows x64. On another
platform, or with a damaged runtime, the launcher still answers: the package says once that it
is inactive, and `craft-review` denies every shell command because its policy cannot be
evaluated. Reinstall the package if `bin/craft` itself is missing.

Sources: [plugins](https://code.claude.com/docs/en/plugins),
[marketplaces](https://code.claude.com/docs/en/plugin-marketplaces),
[project instructions](https://code.claude.com/docs/en/memory),
[changelog](https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md) (2.1.299).

## Codex CLI

```sh
codex plugin marketplace add Aliaksie/engineering-pack --ref distribution
codex plugin add craft-engineering@engineering-pack
codex plugin list
```

Restart Codex, verify the skill appears in `/skills` and invoke
`$craft-engineering-foundations`. Hook-bearing packages install manually; the public plugin
directory does not accept them. For guidance without plugins or hooks, merge the `agents`
overlay into the project; Codex discovers `.agents/skills`.

Plugin hooks need their own trust: open `/hooks`, inspect and trust the current definitions,
and review again after they change. Codex 0.164 and later binds that approval to the plugin's
contents, so after every Craft update it skips all Craft hooks, without a message, until you
trust them again in `/hooks`; a safety policy refuses nothing meanwhile. `craft doctor`
compares each approval with the installed plugin and says when to approve again. Codex 0.150 and later loads no project `AGENTS.md`
in an untrusted project, so the `agents` overlay's guidance is absent until you trust the
project. `/status` and `/statusline` show session usage; `craft`
receives no Codex token counts, so they stay unknown.

Two things leave Craft silent in Codex without an error. A plugin that `config.toml` enables
but `codex plugin add` never installed loads nothing; `craft doctor` names it with the command
to run. Under the `workspace-write` sandbox `.git` is read-only, so a commit the agent makes
fails; add the repository's `.git` to `sandbox_workspace_write.writable_roots` in
`config.toml` if the agent should commit.

Sources: [CLI commands](https://learn.chatgpt.com/docs/cli/reference),
[plugin packaging](https://developers.openai.com/plugins/build/plugins),
[skill discovery](https://learn.chatgpt.com/docs/build-skills),
[hook discovery and trust](https://learn.chatgpt.com/docs/hooks),
[0.164.0](https://github.com/openai/codex/releases/tag/rust-v0.164.0) and
[0.150.0](https://github.com/openai/codex/releases/tag/rust-v0.150.0) release notes,
[CLI hook browser and status controls](https://learn.chatgpt.com/docs/developer-commands?surface=cli).

## Gemini CLI

```sh
gemini extensions install https://github.com/Aliaksie/engineering-pack --ref distribution
gemini extensions list
```

Restart Gemini and verify the extension and its skills are listed. The extension contains
guidance and tools, with no automatically registered hooks or MCP connections. For hooks, use
the `gemini` project overlay instead ([local builds](#local-builds-and-project-overlays)); do
not install both into the same project, since they expose the same skills.

Gemini CLI is for Gemini Code Assist enterprise users only. Free and Google AI Pro/Ultra
users moved to Antigravity CLI on 2026-06-18. Craft has no Antigravity adapter, and importing
Craft packages into Antigravity is untested.

Sources: [extension reference](https://geminicli.com/docs/extensions/reference/),
[Gemini CLI README](https://github.com/google-gemini/gemini-cli).

## JetBrains AI Assistant

1. Download the [distribution ZIP](https://github.com/Aliaksie/engineering-pack/archive/refs/heads/distribution.zip),
   extract it and merge the complete `jetbrains/` folder into the target project, including
   `.aiassistant`, `.agents` and `packages`. Preserve existing files.
2. Open `.aiassistant/rules/engineering-pack.md` and choose **Always** as its rule type.
3. Start a chat and verify the rule appears in the response's attachments.

The rule comes from the foundations skill and links its packaged references; attach one
manually if the chat cannot follow a local link. A rule adds no shell execution, hooks or
enforcement ([project rules](https://www.jetbrains.com/help/ai-assistant/configure-project-rules.html)).
External agents inside JetBrains ([ACP](https://www.jetbrains.com/help/ai-assistant/acp.html))
use their own installation above; an IDE-managed process may use a separate profile.

## First use

Ask the agent: **"Check Craft Capabilities setup, hook activity and session usage."**

Each package's instructions carry one short notice for first use: what the host supports, what
remains unverified and what to ask next. `SETUP.md` and `SETUP.json` serve that setup check.

An enabled plugin or trusted project does not prove hooks run. Do some ordinary work and look
for fresh `host.hook` events in `telemetry/events.jsonl` (`craft settings --root <project>`
shows the shared directory); missing events alone do not tell discovery, trust, disabled telemetry and storage
failures apart.

Where shell execution is available, a status read works without hooks:

```sh
"<package-root>/bin/craft" statusline <host> --package <package> --root "<project-root>" --json
```

Use `bin\craft.cmd` on Windows (`SETUP.json` names it as `sessionAdvice.entry`). Without
`--json` it serves as the Claude Code status line. The status read changes no host settings.

To connect the display and inspect configuration choices, run the installed launcher:

```sh
"<package-root>/bin/craft" host-setup --host claude --package craft-engineering --root "<project-root>"
```

The preview includes exact Claude display settings, supported controls, Craft context
thresholds/budgets, and optional permission suggestions. On macOS/Linux, repeat with `--apply`
to add the display to `.claude/settings.local.json`. Existing fields and custom status lines
are preserved. Setup checks project local, shared project and user settings (including
`CLAUDE_CONFIG_DIR`) in precedence order; a conflicting effective status line requires manual
integration. Session flags and managed host policy still need verification in Claude. Run an
ordinary Claude turn to verify `Context N/M tokens (P%)` appears. The live payload selects its own worktree;
setup does not pin later sessions to the checkout where settings were created.

For another host, change `--host` to its adapter name. Codex terminal CLI uses `/status` and
`/statusline`; its preview also explains `tui.status_line` and
`model_auto_compact_token_limit`. These controls are not assumed to exist in desktop/IDE
surfaces. Other adapters explain their supported controls and remaining gaps. No setup
command changes native auto-compaction or triggers it.

For opt-in command permission rules, run:

```sh
"<package-root>/bin/craft" permissions --host codex
```

Claude and Codex get suggestions scoped to that installed launcher's inspection subcommands.
Review and merge only the rules you want into your own host configuration. They are never
applied by Craft, including with `host-setup --apply`; no consumer build/test or mutating
Craft commands are included. Codex `allow` rules can run matching commands outside the
sandbox without prompting, and arguments remain unrestricted. Preserve existing managed,
ask and deny policies. Unsupported hosts receive a manual fallback, not invented rules.

See [session observations](SESSION.md#host-observations) for native event forwarding,
separate token measurements and compaction verification.

## Updates

| Host | Update |
|---|---|
| Claude Code | `claude plugin marketplace update craft-capabilities`, then `claude plugin update <package>@craft-capabilities` per installed package |
| Codex | `codex plugin marketplace upgrade engineering-pack` |
| Gemini | `gemini extensions update engineering-pack`, then restart |
| Overlays | Review and merge regenerated files, preserving consumer changes |

Verify the skills in a fresh session. Rolling updates can keep the same version; if a host
skips it, reinstall the plugin. Rebuilding a local Codex marketplace does not refresh cached
installations; refresh each package and review hook trust again. See [releasing](RELEASING.md).

## Checking an installation

`bin/craft doctor` (from any installed package, or `craft doctor --root <project>`) reports
for the project whether each host would run Craft, from files only:

- Claude Code: Craft packages in the install record that apply to the project, whether
  user, project or local settings enable them, a bundled runtime for this platform matching
  its manifest digest, and Craft hook commands also registered in settings files;
- Codex: configured and enabled packages at the newest cached version (older cached
  versions are listed, not judged), packages the configuration enables that are not in the
  plugin cache, project trust, the hooks feature switch, a recorded
  approval for each hook a package declares and whether it was given to the plugin content
  now installed (an update leaves approvals behind), the bundled runtime, and Craft commands
  in other hook files;
- Cursor, Copilot CLI and Gemini CLI project overlays: the bundled runtime each command
  starts;
- per host and package, the last hook event Craft recorded in this project and the version
  it ran, so a session that predates an update is named and an event of another version is
  not taken for the installed one; for a host with no install found, the last event recorded
  for it;
- the configuration layers: a file that cannot be used, fields of the organization or user
  file this build does not know and left out, and a safety policy that is on while no hook of
  an installed host is known to have run in the project.

It writes nothing to host configuration or task state and starts no host. Whether a running
session loaded the current install, whether a Codex hook's declaration still matches its
approval, and managed or command-line settings stay unknown. It exits 1 only when an enabled package cannot run here
(missing, unrunnable or altered bundled runtime); `--json` returns the full report.

## Local builds and project overlays

From a source checkout (see [contributing](CONTRIBUTING.md) for prerequisites):

```sh
git clone https://github.com/Aliaksie/engineering-pack.git
cd engineering-pack
npm run build
```

Building installs, trusts and connects nothing. `npm run build:distribution` assembles
`dist/distribution` with the published layout.

| Host | Generated directory | How to adopt it |
|---|---|---|
| Claude Code | `dist/claude` | `claude plugin marketplace add ./dist/claude`, then install plugins |
| Codex | `dist/codex` | `codex plugin marketplace add ./dist/codex`, then add packages |
| Cursor | `dist/cursor` | Merge `.cursor/skills`, hook configuration and supporting resources into the project |
| Copilot | `dist/copilot` | Merge `.github/skills`, instructions, hook configuration and supporting resources |
| Gemini CLI | `dist/gemini` | Merge `.gemini/skills`, settings, GEMINI.md and supporting resources |
| Gemini extension | `dist/gemini-extension` | `gemini extensions install ./dist/gemini-extension`; guidance only |
| JetBrains AI Chat | `dist/jetbrains` | Merge the project rule and resources; select Always in the IDE |
| AGENTS / Agent Skills | `dist/agents` | Merge `.agents/skills` and resources; merge the instruction index into your own instructions |

`npm run build` writes exports without executables. To run commands and hooks locally, add
the one for the current platform:

```sh
npm run build:native
node tooling/native-install.mjs dist/project-evidence dist/claude --current-target
```

The install step records the source revision and refuses executables built from uncommitted
changes, so run it from a clean checkout.

Before merging an overlay into a project:

1. Include hidden skill and configuration directories; copying only visible files omits
   discovery metadata.
2. Preserve the relative layout, including `packages/`, shared `docs/`, `schemas/` and license
   files. Skills link to package resources and hook commands reference package paths.
3. Merge settings, hooks and instruction files with existing content, preserving the project's
   `AGENTS.md`, `.github/copilot-instructions.md` and `GEMINI.md`.
4. Check `adapter-report.json` ([adapters](ADAPTERS.md)), then try one workflow in a
   disposable project before adopting hooks in a real one.

Copilot CLI skips repository hooks in a folder it does not trust; Craft's overlay hooks run
only once the project is trusted.

Optional project configuration lives in `.craft/project.json`; see [routing](ROUTING.md) and
[integrations](INTEGRATIONS.md).
