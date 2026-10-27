use super::*;
use serde_json::json;

const ID: &str = "0123456789abcdef01234567";
const HEX: &str = "89abcdef0123456789abcdef";
const WORKSPACE: &str = "4453294baf54499f6d52bbe0218b949b9fb1d4d4579e7fcf92800fa059eb640d";
const NOW: i64 = 100 * DAY_MS;
const OLD: i64 = NOW - SESSION_RETENTION_DAYS * DAY_MS;

fn entry(name: &str, modified: i64) -> Entry {
    Entry {
        name: name.into(),
        file: true,
        bytes: 10,
        modified,
    }
}

fn session<'a>(layout: Layout, entries: &'a [Entry], described: Described) -> Session<'a> {
    Session {
        id: ID,
        layout,
        entries,
        complete: true,
        modified: OLD,
        staged: false,
        described,
    }
}

fn current(entries: &[Entry]) -> Session<'_> {
    session(
        Layout::Current,
        entries,
        Described::Workspace(WORKSPACE.into()),
    )
}

#[test]
fn idle_recognized_session_is_removable() {
    let entries = [
        entry("manifest.json", OLD),
        entry("checkpoint.json", OLD),
        entry("task.json", OLD),
        entry(&format!("activity-{HEX}.json"), OLD),
        entry(&format!("workers-{HEX}.json"), OLD),
        entry(&format!("control-{HEX}.json"), OLD),
        entry(&format!("routing-{HEX}.json"), OLD),
        entry(&format!("routing-{HEX}.json.generation"), OLD),
    ];
    let session = current(&entries);
    assert_eq!(keep(&session, NOW, &Workspaces::default()), None);
    assert_eq!(session.bytes(), 80);
    assert_eq!(idle_days(&session, NOW), SESSION_RETENTION_DAYS);
}

#[test]
fn previous_layout_names_are_recognized_only_there() {
    let entries = [
        entry(&format!("{ID}.json"), OLD),
        entry(&format!("active-{HEX}.json"), OLD),
        entry(&format!("session-{HEX}.json"), OLD),
        entry(&format!("native-control-{HEX}.json"), OLD),
        entry(&format!("migration-{HEX}.json"), OLD),
        entry(&format!("routing-{HEX}.json.generation"), OLD),
    ];
    let previous = session(Layout::Previous, &entries, Described::Absent);
    assert_eq!(keep(&previous, NOW, &Workspaces::default()), None);
    let mismatched = current(&entries);
    assert_eq!(
        keep(&mismatched, NOW, &Workspaces::default()),
        Some("holds an unrecognized file")
    );
}

#[test]
fn any_recent_write_keeps_the_session() {
    let recent = NOW - SESSION_RETENTION_DAYS * DAY_MS + 1;
    let entries = [
        entry("manifest.json", OLD),
        entry("checkpoint.json", recent),
    ];
    assert_eq!(
        keep(&current(&entries), NOW, &Workspaces::default()),
        Some("written within the retention period")
    );
    let entries = [entry("manifest.json", OLD)];
    let mut touched = current(&entries);
    touched.modified = recent;
    assert_eq!(
        keep(&touched, NOW, &Workspaces::default()),
        Some("written within the retention period")
    );
    // A cleanup's own removals touch a staged directory; its entries still decide.
    touched.staged = true;
    assert_eq!(keep(&touched, NOW, &Workspaces::default()), None);
    let mut emptied = current(&[]);
    (emptied.staged, emptied.modified) = (true, recent);
    assert_eq!(keep(&emptied, NOW, &Workspaces::default()), None);
    let entries = [entry("checkpoint.json", recent)];
    let mut written = current(&entries);
    written.staged = true;
    assert_eq!(
        keep(&written, NOW, &Workspaces::default()),
        Some("written within the retention period")
    );
}

#[test]
fn held_unknown_or_irregular_entries_keep_the_session() {
    let none = Workspaces::default();
    for (name, file, reason) in [
        ("checkpoint.json.lock", true, "held by a writer"),
        ("task.json.lease-guard", true, "held by a writer"),
        (
            "checkpoint.json",
            false,
            "holds an entry that is not a regular file",
        ),
        ("notes.txt", true, "holds an unrecognized file"),
        ("activity-XYZ.json", true, "holds an unrecognized file"),
        (
            "checkpoint.json.123-4-0.tmp",
            true,
            "holds an unrecognized file",
        ),
    ] {
        let entries = [Entry {
            file,
            ..entry(name, OLD)
        }];
        assert_eq!(keep(&current(&entries), NOW, &none), Some(reason), "{name}");
    }
    let entries = [entry("manifest.json", OLD)];
    let mut saturated = current(&entries);
    saturated.complete = false;
    assert_eq!(
        keep(&saturated, NOW, &none),
        Some("too many entries to inspect")
    );
}

#[test]
fn open_or_unverified_workspaces_keep_the_session() {
    let entries = [entry("manifest.json", OLD)];
    let other = Workspaces {
        active: HashSet::from(["f".repeat(64)]),
        unverified: false,
    };
    let own = Workspaces {
        active: HashSet::from([WORKSPACE.to_string()]),
        unverified: false,
    };
    let unverified = Workspaces {
        unverified: true,
        ..Workspaces::default()
    };
    let open = Some("a workspace may still be open");
    assert_eq!(keep(&current(&entries), NOW, &other), None);
    assert_eq!(keep(&current(&entries), NOW, &own), open);
    assert_eq!(keep(&current(&entries), NOW, &unverified), open);
    // Without a manifest the session cannot be told apart from any open workspace's.
    let undescribed = session(Layout::Current, &entries, Described::Absent);
    assert_eq!(keep(&undescribed, NOW, &other), open);
    assert_eq!(keep(&undescribed, NOW, &Workspaces::default()), None);
    let invalid = session(Layout::Current, &entries, Described::Invalid);
    assert_eq!(
        keep(&invalid, NOW, &Workspaces::default()),
        Some("manifest is invalid")
    );
}

#[test]
fn manifest_description_requires_its_format() {
    assert_eq!(described(None), Described::Absent);
    let manifest = json!({"formatVersion": 1, "created": 1, "workspace": WORKSPACE});
    assert_eq!(
        described(Some(&manifest)),
        Described::Workspace(WORKSPACE.into())
    );
    for invalid in [
        json!({"formatVersion": 2, "workspace": WORKSPACE}),
        json!({"formatVersion": 1, "workspace": WORKSPACE.to_uppercase()}),
        json!({"formatVersion": 1}),
        json!([]),
    ] {
        assert_eq!(described(Some(&invalid)), Described::Invalid);
    }
}

#[test]
fn previous_shared_entries_are_classified_by_name() {
    for (name, expected) in [
        ("telemetry.jsonl", Leftover::Record),
        ("telemetry.jsonl.1", Leftover::Record),
        ("telemetry-key.json", Leftover::Record),
        ("telemetry-captures.json", Leftover::Record),
        ("migration-0123456789abcdef01234567.json", Leftover::Receipt),
        ("migration-short.json", Leftover::Unknown),
        ("project-evidence.sqlite", Leftover::Database),
        ("project-evidence.sqlite-wal", Leftover::Database),
        ("project-evidence.sqlite.backup", Leftover::Database),
        // The database lock is an OS lock whose file outlives its holder.
        ("project-evidence.sqlite.lock", Leftover::Database),
        ("telemetry-key.json.lock", Leftover::Lock),
        ("workspace-guard.json.lock", Leftover::Lock),
        ("sessions", Leftover::Workspaces),
        ("workspace-guard.json", Leftover::Guard),
        ("notes.txt", Leftover::Unknown),
    ] {
        assert_eq!(leftover(name), expected, "{name}");
    }
}
