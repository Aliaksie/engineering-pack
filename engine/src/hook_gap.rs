//! Host sessions that ran in a project without Craft's hooks. A host that binds hook trust to
//! a plugin's content (Codex) skips every Craft hook after an update until the user trusts
//! them again, and some of its surfaces say nothing; while they are skipped, Craft cannot
//! speak. The host's own session records against Craft's hook telemetry show the gap once a
//! hook runs again, or to `craft doctor`.
use crate::timestamp;
use serde_json::{Value, json};
use std::collections::HashSet;

/// Where each host lets the user trust a plugin's hooks.
pub fn trust_place(host: &str) -> &'static str {
    match host {
        "codex" => {
            "`/hooks` in the terminal or Settings → Hooks in the Codex app and IDE extension (`codex exec` cannot ask)"
        }
        _ => "the host's hook settings",
    }
}

fn host_name(host: &str) -> &str {
    match host {
        "codex" => "Codex",
        "claude" => "Claude Code",
        other => other,
    }
}

/// The gap over `{host, current?, sessions: [{identity, started}], records, installed?,
/// version?, safety}`: `records` are retained telemetry records, `sessions` the host's own
/// session records for this project as Craft pseudonyms with their start in ms, `current`
/// the pseudonym of the session asking (left out of the count), `installed` when the running
/// bundle was installed. A session counts when it started within the retained telemetry and
/// after the last hook of another session on this host, and after any earlier hook of the
/// asking session itself (resumed, it heard the gap then), and no hook record names it. With
/// no such hook, only sessions after the install count: before it, Craft may not have been
/// installed at all. `None` when none counts.
pub fn gap(facts: &Value) -> Option<Value> {
    let host = facts["host"].as_str()?;
    let current = facts["current"].as_str();
    let records = facts["records"].as_array()?;
    let mut hooked = HashSet::new();
    let mut window: Option<i64> = None;
    let mut last: Option<(i64, &Value)> = None;
    // The current session's own earlier hooks (a resumed session) already heard any gap.
    let mut own: Option<i64> = None;
    for record in records {
        let Some(at) = record["timestamp"].as_str().and_then(timestamp::parse) else {
            continue;
        };
        window = Some(window.map_or(at, |first| first.min(at)));
        if record["name"] != "host.hook" || record["host"] != host {
            continue;
        }
        let session = record["sessionId"].as_str();
        if let Some(session) = session {
            hooked.insert(session);
        }
        if session.is_some() && session == current {
            own = own.max(Some(at));
        } else if last.is_none_or(|(seen, _)| at > seen) {
            last = Some((at, &record["packVersion"]));
        }
    }
    // Nothing retained: no baseline to say a session lacked a hook.
    let window = window?;
    let since = window
        .max(
            last.map(|(at, _)| at)
                .or(facts["installed"].as_i64())
                .unwrap_or(window),
        )
        .max(own.unwrap_or(window));
    let mut starts: Vec<i64> = facts["sessions"]
        .as_array()?
        .iter()
        .filter_map(|session| {
            let identity = session["identity"].as_str()?;
            let started = session["started"].as_i64()?;
            (started > since && Some(identity) != current && !hooked.contains(identity))
                .then_some(started)
        })
        .collect();
    starts.sort_unstable();
    let first = *starts.first()?;
    let mut found = json!({
        "host": host, "sessions": starts.len(), "first": timestamp::format(first),
        "safety": facts["safety"] == true,
    });
    if let (Some(previous), Some(version)) = (
        last.and_then(|(_, version)| version.as_str()),
        facts["version"].as_str(),
    ) && previous != version
    {
        found["previousVersion"] = json!(previous);
        found["version"] = json!(version);
    }
    Some(found)
}

/// One line for the user on a gap `found` by [`gap`].
pub fn line(found: &Value) -> String {
    let host = found["host"].as_str().unwrap_or_default();
    let count = found["sessions"].as_u64().unwrap_or(0);
    let first = found["first"].as_str().unwrap_or_default();
    let day = first.get(..16).map_or(first.to_owned(), |minute| {
        format!("{} UTC", minute.replace('T', " "))
    });
    let mut text = format!(
        "Craft: {count} earlier {} session{} in this project since {day} ran without Craft's hooks",
        host_name(host),
        if count == 1 { "" } else { "s" },
    );
    if let (Some(previous), Some(version)) =
        (found["previousVersion"].as_str(), found["version"].as_str())
    {
        text.push_str(&format!(
            " (Craft was updated from {previous} to {version})"
        ));
    }
    text.push('.');
    if host == "codex" {
        text.push_str(
            " Codex skips a plugin's hooks until they are trusted, and again after every plugin update",
        );
    } else {
        text.push_str(&format!(" {} did not run them", host_name(host)));
    }
    text.push_str(&format!(": trust them with {}.", trust_place(host)));
    if found["safety"] == true {
        text.push_str(" The safety policy checked no command in those sessions.");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hook(session: &str, at: &str, version: &str) -> Value {
        json!({"name": "host.hook", "host": "codex", "sessionId": session, "timestamp": at, "packVersion": version})
    }

    fn session(identity: &str, at: &str) -> Value {
        json!({"identity": identity, "started": timestamp::parse(at).unwrap()})
    }

    fn facts(records: Vec<Value>, sessions: Vec<Value>) -> Value {
        json!({
            "host": "codex", "current": "now", "records": records, "sessions": sessions,
            "installed": timestamp::parse("2026-11-12T08:00:00.000Z"), "version": "0.0.7", "safety": true,
        })
    }

    #[test]
    fn sessions_after_the_update_without_a_hook_are_counted_once_hooks_run_again() {
        // The asking session's own hook is recorded after its decision.
        let records = vec![hook("old", "2026-11-10T09:00:00.000Z", "0.0.6")];
        let sessions = vec![
            session("old", "2026-11-10T08:59:00.000Z"),
            session("a", "2026-11-12T10:00:00.000Z"),
            session("b", "2026-11-12T11:30:00.000Z"),
            session("now", "2026-11-13T08:58:00.000Z"),
        ];
        let found = gap(&facts(records, sessions)).unwrap();
        assert_eq!(found["sessions"], 2, "{found}");
        assert_eq!(found["first"], "2026-11-12T10:00:00.000Z");
        assert_eq!(found["previousVersion"], "0.0.6");
        let text = line(&found);
        assert!(
            text.starts_with("Craft: 2 earlier Codex sessions in this project since 2026-11-12 10:00 UTC ran without Craft's hooks (Craft was updated from 0.0.6 to 0.0.7)."),
            "{text}"
        );
        assert!(text.contains("Settings → Hooks"), "{text}");
        assert!(text.ends_with("The safety policy checked no command in those sessions."));
    }

    #[test]
    fn the_next_session_does_not_hear_it_again() {
        // The session that heard it recorded hooks; the next one starts after them.
        let records = vec![
            hook("old", "2026-11-10T09:00:00.000Z", "0.0.6"),
            hook("heard", "2026-11-13T09:00:00.000Z", "0.0.7"),
        ];
        let sessions = vec![
            session("a", "2026-11-12T10:00:00.000Z"),
            session("heard", "2026-11-13T08:58:00.000Z"),
        ];
        assert_eq!(gap(&facts(records, sessions)), None);
    }

    #[test]
    fn nothing_is_claimed_without_retained_telemetry_or_before_it() {
        let sessions = vec![session("a", "2026-11-12T10:00:00.000Z")];
        assert_eq!(gap(&facts(vec![], sessions.clone())), None);
        // Records retained only from after the session: no baseline for it.
        let records = vec![hook("x", "2026-11-12T12:00:00.000Z", "0.0.7")];
        let mut asked = facts(records, sessions);
        asked["current"] = Value::Null;
        assert_eq!(gap(&asked), None);
    }

    #[test]
    fn without_a_version_change_the_cause_is_not_claimed() {
        let records = vec![hook("old", "2026-11-12T09:00:00.000Z", "0.0.7")];
        let sessions = vec![session("a", "2026-11-12T10:00:00.000Z")];
        let mut asked = facts(records, sessions);
        asked["safety"] = json!(false);
        let found = gap(&asked).unwrap();
        assert_eq!(found.get("previousVersion"), None);
        let text = line(&found);
        assert!(
            text.starts_with("Craft: 1 earlier Codex session in this project since 2026-11-12 10:00 UTC ran without Craft's hooks. Codex skips"),
            "{text}"
        );
        assert!(!text.contains("safety"), "{text}");
    }

    #[test]
    fn without_an_earlier_hook_of_the_host_only_sessions_after_the_install_count() {
        let mut claude = hook("c", "2026-11-01T09:00:00.000Z", "0.0.7");
        claude["host"] = json!("claude");
        let sessions = vec![
            // Codex used before Craft was installed for it.
            session("before", "2026-11-05T10:00:00.000Z"),
            session("a", "2026-11-12T10:00:00.000Z"),
        ];
        let found = gap(&facts(vec![claude], sessions)).unwrap();
        assert_eq!(found["sessions"], 1, "{found}");
        assert_eq!(found.get("previousVersion"), None);
    }

    #[test]
    fn a_resumed_session_that_already_heard_it_does_not_hear_it_again() {
        let records = vec![
            hook("old", "2026-11-10T09:00:00.000Z", "0.0.6"),
            hook("now", "2026-11-13T09:00:00.000Z", "0.0.7"),
        ];
        let sessions = vec![
            session("a", "2026-11-12T10:00:00.000Z"),
            session("now", "2026-11-13T08:58:00.000Z"),
        ];
        assert_eq!(gap(&facts(records, sessions)), None);
    }

    #[test]
    fn hooks_of_another_host_do_not_close_the_gap() {
        let mut claude = hook("a", "2026-11-12T10:01:00.000Z", "0.0.7");
        claude["host"] = json!("claude");
        let records = vec![hook("old", "2026-11-12T09:00:00.000Z", "0.0.7"), claude];
        let sessions = vec![session("a", "2026-11-12T10:00:00.000Z")];
        assert_eq!(gap(&facts(records, sessions)).unwrap()["sessions"], 1);
    }
}
