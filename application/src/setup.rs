//! Host setup previews and explicitly applied Claude display configuration. B11 permission
//! proposals are output only: no host permissions, trust or compaction settings are changed.
use crate::{failure::Failure, statusline};
use craft_platform::{host_settings, state};
use serde_json::{Value, json};
use std::path::Path;

const COMMANDS: &[&str] = &[
    "permissions",
    "settings",
    "route",
    "session",
    "session-advice",
    "context-report",
    "locate",
    "lookup",
    "checkpoint-read",
];

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}
fn single_line(value: &str) -> bool {
    !value.is_empty() && value.len() <= 4096 && !value.chars().any(|c| c.is_control())
}

fn launcher(bundle: &Path) -> Result<String, Failure> {
    let path = state::physical(&bundle.join("bin/craft"))?;
    craft_platform::files::read_bounded(&path, 65_536)
        .map_err(|_| "setup requires the installed package's bounded launcher file")?;
    let value = path
        .to_str()
        .filter(|p| single_line(p))
        .ok_or("setup requires a single-line UTF-8 launcher path")?;
    Ok(value.into())
}

/// Suggestions are intentionally restricted to inspection commands, each with its own prefix.
/// These commands can write operational telemetry; they cannot run consumer build/test commands.
pub(crate) fn permissions(host: &str, bundle: &Path) -> Result<Value, Failure> {
    statusline::capabilities(host).ok_or("unknown setup host")?;
    let executable = launcher(bundle)?;
    let mut result = json!({"status":"unsupported","applied":false,"host":host,"commands":COMMANDS,
        "effect":"Optional host permission grants for inspection commands; local operational state, caches and telemetry may be written. No consumer build/test, hook trust, compaction control or mutating Craft command is included.",
        "next":"Review these suggestions in the actual host surface before changing your own permission settings."});
    if cfg!(windows) {
        result["reason"] = json!(
            "Permission suggestions target the POSIX launcher; use the Windows host's native approval controls for bin/craft.cmd."
        );
        return Ok(result);
    }
    match host {
        "claude" => {
            // Claude's rule language interprets glob and delimiter characters even inside
            // shell quotes. Do not produce a rule broader than this literal installation.
            if executable.contains(['*', '?', '[', ']', '(', ')', '\\']) {
                result["reason"] = json!(
                    "Launcher path cannot be represented as a literal Claude permission prefix; use /permissions for this installation."
                );
                return Ok(result);
            }
            let rules: Vec<String> = COMMANDS
                .iter()
                .map(|command| format!("Bash({} {command} *)", quote(&executable)))
                .collect();
            result["status"] = json!("suggested");
            result["file"] = json!(".claude/settings.local.json");
            result["merge"] = json!({"permissions":{"allow":rules}});
            result["source"] = json!("https://code.claude.com/docs/en/permissions");
        }
        "codex" => {
            let rules: Vec<String> = COMMANDS
                .iter()
                .map(|command| {
                    format!(
                        "prefix_rule(pattern = [{}, {}], decision = \"allow\")",
                        json!(executable),
                        json!(command)
                    )
                })
                .collect();
            result["status"] = json!("suggested");
            result["file"] = json!(
                ".codex/rules/craft.rules (trusted project config layer; alternatively your user rules directory)"
            );
            result["content"] = json!(format!("{}\n", rules.join("\n")));
            result["effect"] = json!(
                "Codex allow rules can run these prefixes outside the sandbox without prompting; arguments remain unrestricted. Review paths, telemetry settings and managed policy before opting in."
            );
            result["source"] = json!("https://learn.chatgpt.com/docs/agent-configuration/rules");
        }
        _ => {
            result["reason"] = json!(
                "No verified permission-rule adapter; use the host's own permission controls."
            );
        }
    }
    Ok(result)
}

pub(crate) fn run(
    root: &Path,
    host: &str,
    package: &str,
    bundle: &Path,
    apply: bool,
) -> Result<Value, Failure> {
    let capabilities = statusline::capabilities(host).ok_or("unknown setup host")?;
    if !single_line(package)
        || !package
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err("setup requires a package name".into());
    }
    if apply && host != "claude" {
        return Err("automatic display setup is supported only for Claude Code; inspect the native host controls in the preview".into());
    }
    let root = state::canonical_root(root)?;
    let executable = launcher(bundle)?;
    let _project = root
        .to_str()
        .filter(|p| single_line(p))
        .ok_or("setup requires a single-line UTF-8 project path")?;
    let mut report = json!({"formatVersion":1,"host":host,"package":package,"mode":if apply {"apply-display"} else {"preview"},
        "capabilities":capabilities,"connection":"unverified","usage":"unknown until a fresh native observation arrives",
        "configuration":[{"file":".craft/project.json","keys":["efficiency.context.elevatedAt","efficiency.context.compactAt","efficiency.context.restartAt","efficiency.context.action","execution.hosts.<host>.contextLimits.<exact-model>.windowTokens","execution.hosts.<host>.contextLimits.<exact-model>.budgetTokens"],
            "purpose":"Craft advice thresholds and optional per-model working budget. Window limits cannot establish current usage or change the model's actual capacity."}],
        "permissions":permissions(host,bundle)?,
        "compaction":{"nativeAutoCompact":capabilities["nativeAutoCompact"],"control":"not-connected","completion":"not-observed","next":"Use native auto-compaction or the host's manual command. Preserve a task checkpoint; verify continuation after compaction. Craft does not trigger compaction through setup."}});
    match host {
        "claude" => {
            if cfg!(windows) {
                if apply {
                    return Err(
                        "use the preview's manual statusline guidance for Claude on Windows".into(),
                    );
                }
                report["display"] = json!({"status":"manual","surface":"Claude Code on Windows","next":"Use the native /statusline setup and the documented PowerShell or Git Bash configuration for bin/craft.cmd.","source":"https://code.claude.com/docs/en/statusline#windows-configuration"});
                return Ok(report);
            }
            let command = format!(
                "{} statusline claude --package {} --native-stdin",
                quote(&executable),
                quote(package)
            );
            let display = json!({"type":"command","command":command});
            report["display"] = json!({"status":"available","surface":"Claude Code command status line (POSIX shell)","current":host_settings::inspect(&root)?,
                "file":root.join(".claude/settings.local.json"),"merge":{"statusLine":display},
                "next":"Review the preview, then run host-setup with the same arguments and --apply. An existing custom statusLine is preserved; integrate it manually. Verify a real turn shows Context N/M tokens.",
                "source":"https://code.claude.com/docs/en/statusline"});
            if apply {
                report["display"]["application"] = host_settings::apply(&root, &display)?;
            }
        }
        "codex" => {
            report["display"] = json!({"status":"native-ui","surface":"Codex terminal CLI; desktop/IDE controls must be checked separately","inspect":"/status","configure":"/statusline",
                "next":"Enable context and token items in the native status-line selector. Plugin hooks do not supply these counts to Craft."});
            report["configuration"].as_array_mut().unwrap().push(json!({"file":"Codex active config.toml layer","keys":["tui.status_line","model_auto_compact_token_limit","model_auto_compact_token_limit_scope"],
                "purpose":"Native CLI display and automatic compaction threshold. Unset threshold uses model defaults; Craft does not change these settings.","source":"https://learn.chatgpt.com/docs/config-file/config-reference"}));
            report["integration"] = json!({"status":"requires-connected-app-server-client","usageEvent":"thread/tokenUsage/updated","compactionEvents":["item/started contextCompaction","item/completed contextCompaction"],
                "ingest":"host-observe --host codex --session <thread-id> --data native-event.json --observed-at <transport-receive-time>",
                "next":"The client that owns the app-server connection must forward native events. Installing the plugin does not attach to a running app-server. No transcript scraping is performed.","source":"https://learn.chatgpt.com/docs/app-server"});
        }
        _ => {
            report["display"] = json!({"status":"manual","commands":capabilities["commands"],"next":"Use the mapped native controls on the verified host surface. This adapter cannot install a persistent usage display; explicit status reads leave unavailable counts unknown."});
        }
    }
    Ok(report)
}
