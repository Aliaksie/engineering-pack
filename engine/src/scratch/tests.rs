use super::*;
use crate::contracts::effective_settings;

const SESSION: &str = "d4cd9053a12d5c8ed47e7b71a96e426a7ba4be9cf2deb5ae0b514f1c6749ed1d";
const TASK: &str = "c44c3ed46d08bd5a9c2f79fda4c57e211b8d6c7cc2e22b1a522d90ba54f75da2";
const KEY: &str = "4453294baf54499f6d52bbe0218b949b9fb1d4d4579e7fcf92800fa059eb640d";

fn policy(workspace: Value) -> Policy {
    let project = json!({"formatVersion": 1, "workspace": workspace});
    Policy::from_settings(
        &effective_settings(&project, &json!({}), &json!({})).unwrap()["settings"],
    )
}

fn manifest_at(updated: i64, closed: bool) -> Value {
    json!({"owner": "craft", "formatVersion": 1, "project": "p", "session": SESSION, "task": TASK,
        "updated": updated, "closed": closed, "operations": {}, "evidence": []})
}

#[test]
fn default_policy_matches_documented_defaults() {
    let defaults = policy(json!({}));
    assert!(defaults.enabled && defaults.evidence_enabled);
    assert_eq!(defaults.max_sessions, 20);
    assert_eq!(defaults.stale_ms, 24 * HOUR_MS);
    assert_eq!(
        (defaults.scratch_bytes, defaults.scratch_files),
        (8_388_608, 64)
    );
    assert_eq!(defaults.retention_ms, 7 * DAY_MS);
    assert_eq!(
        (defaults.evidence_artifacts, defaults.evidence_bytes),
        (16, 1_048_576)
    );
    let custom = policy(
        json!({"enabled": false, "scratch": {"staleAfterHours": 2}, "evidence": {"retentionDays": 1}}),
    );
    assert!(!custom.enabled);
    assert_eq!(
        (custom.stale_ms, custom.retention_ms),
        (2 * HOUR_MS, DAY_MS)
    );
}

#[test]
fn identity_matches_the_js_digests() {
    let bound = Identity::new("session", "task", Some(&json!(["kit"])), None);
    assert_eq!(
        (bound.session.as_str(), bound.task.as_str()),
        (SESSION, TASK)
    );
    assert_eq!(bound.key(), KEY);
    assert_eq!(
        bound.operation,
        "e4a9e4d8baa71a9d8ebf56ec8ae3ea1e82be9f96ae6fe89e36d701d4a5337f30"
    );
    let plain = Identity::new("session", "task", None, Some("worker-1"));
    assert_eq!(
        plain.task,
        "b9ce06a01002b74d199af6ebe3f5fb94ceba07ec633822e44fec47b29b9edb7d"
    );
    assert_eq!(
        plain.operation,
        "e50fbe76491523bde76bfa56b0787f22343a91cede914adee72fef56e9ff11bd"
    );
    assert_eq!(
        plain.key(),
        "6e9a0a1bc3f873b3be55d97ee670356c6307aaeb48ad4d4db8e89ab7d42e7b72"
    );
}

#[test]
fn open_operations_hold_the_workspace_until_stale() {
    let policy = policy(json!({}));
    let mut open = manifest_at(1_000, false);
    open["operations"] = json!({"a": {"pid": 1, "closed": false}});
    assert!(active(&open, 1_000 + policy.stale_ms - 1, &policy));
    assert!(
        !active(&open, 1_000 + policy.stale_ms, &policy),
        "an abandoned operation never holds forever"
    );
    open["operations"]["a"]["closed"] = json!(true);
    assert!(
        !active(&open, 1_000, &policy),
        "closed operations hold nothing"
    );
}

#[test]
fn scratch_and_evidence_expire_at_their_boundaries() {
    let policy = policy(json!({}));
    let open = manifest_at(0, false);
    assert!(!scratch_expired(&open, policy.stale_ms - 1, &policy));
    assert!(scratch_expired(&open, policy.stale_ms, &policy));
    assert!(
        scratch_expired(&manifest_at(0, true), 0, &policy),
        "closed scratch is disposable at once"
    );
    assert!(!evidence_expired(&open, policy.retention_ms - 1, &policy));
    assert!(evidence_expired(&open, policy.retention_ms, &policy));
}

#[test]
fn names_are_single_safe_segments() {
    for name in ["a", "result.json", "A-b_c.d", &"x".repeat(128)] {
        assert!(is_name(&json!(name)), "{name}");
    }
    for name in [
        "",
        ".hidden",
        "-x",
        "_x",
        "../x",
        "a/b",
        "a b",
        "ü",
        &"x".repeat(129),
    ] {
        assert!(!is_name(&json!(name)), "{name}");
    }
    assert!(!is_name(&json!(5)));
    assert!(is_digest(KEY));
    assert!(!is_digest(&KEY.to_uppercase()));
    assert!(!is_digest(&KEY[1..]));
}

#[test]
fn manifests_validate_every_field() {
    let mut valid = manifest_at(5, false);
    valid["operations"] = json!({SESSION: {"pid": 1, "closed": false}});
    valid["evidence"] = json!([{"file": "x-r", "at": -1, "bytes": 0, "purpose": "acceptance"}]);
    assert_eq!(validate(&valid, "p", KEY), Ok(()));
    assert!(validate(&valid, "other", KEY).is_err());
    assert!(validate(&valid, "p", TASK).is_err());
    let mutations: &[(&str, Value)] = &[
        ("/owner", json!("someone-else")),
        ("/formatVersion", json!(2)),
        ("/session", json!("short")),
        ("/task", json!(null)),
        ("/updated", json!(-1)),
        ("/updated", json!(1.5)),
        ("/closed", json!("no")),
        ("/operations", json!([])),
        ("/evidence", json!({})),
        (
            "/operations/d4cd9053a12d5c8ed47e7b71a96e426a7ba4be9cf2deb5ae0b514f1c6749ed1d/pid",
            json!(0),
        ),
        (
            "/operations/d4cd9053a12d5c8ed47e7b71a96e426a7ba4be9cf2deb5ae0b514f1c6749ed1d/closed",
            json!(null),
        ),
        ("/evidence/0/file", json!("../x")),
        ("/evidence/0/at", json!(null)),
        ("/evidence/0/bytes", json!(-1)),
    ];
    for (pointer, value) in mutations {
        let mut changed = valid.clone();
        *changed.pointer_mut(pointer).unwrap() = value.clone();
        assert!(validate(&changed, "p", KEY).is_err(), "{pointer} = {value}");
    }
    let mut foreign = valid.clone();
    foreign["operations"] = json!({"main": {"pid": 1, "closed": false}});
    assert!(
        validate(&foreign, "p", KEY).is_err(),
        "operation ids are digests"
    );
    let mut full = valid.clone();
    full["operations"] = Value::Object(
        (0..=MANIFEST_ENTRIES)
            .map(|i| (crate::hash(&[i as u8]), json!({"pid": 1, "closed": true})))
            .collect(),
    );
    assert!(validate(&full, "p", KEY).is_err());
    full["operations"] = json!({});
    full["evidence"] = Value::Array(vec![valid["evidence"][0].clone(); MANIFEST_ENTRIES + 1]);
    assert!(validate(&full, "p", KEY).is_err());
    assert!(validate(&Value::Null, "p", KEY).is_err());
}

#[test]
fn new_manifests_validate_and_list_open_operations() {
    let identity = Identity::new("session", "task", Some(&json!(["kit"])), None);
    let mut created = manifest("p", &identity, 7);
    assert_eq!(validate(&created, "p", KEY), Ok(()));
    created["operations"] =
        json!({"a": {"pid": 3, "closed": false}, "b": {"pid": 4, "closed": true}});
    assert_eq!(open_operations(&created), vec!["a".to_string()]);
}

#[test]
fn artifacts_and_promotions_are_bounded() {
    assert_eq!(
        artifact(&json!({"name": "r.json", "content": ""})),
        Ok(("r.json", ""))
    );
    assert!(artifact(&json!({"name": "x".repeat(100), "content": "x"})).is_ok());
    for input in [
        json!({"name": "x".repeat(101), "content": "x"}),
        json!({"name": "r"}),
        json!({"name": "r", "content": 1}),
        json!({"content": "x"}),
    ] {
        assert!(artifact(&input).is_err(), "{input}");
    }
    let policy = policy(json!({"evidence": {"maxArtifacts": 1}}));
    let empty = manifest_at(0, false);
    assert_eq!(
        promotion(
            &json!({"name": "r", "purpose": "acceptance"}),
            &empty,
            &policy
        ),
        Ok("r")
    );
    for input in [
        json!({"name": "r", "purpose": "two words"}),
        json!({"name": "r"}),
        json!({"name": "x".repeat(101), "purpose": "p"}),
    ] {
        assert!(promotion(&input, &empty, &policy).is_err(), "{input}");
    }
    let mut full = empty.clone();
    full["evidence"] = json!([{"file": "a", "at": 0, "bytes": 0}]);
    assert!(
        promotion(
            &json!({"name": "r", "purpose": "acceptance"}),
            &full,
            &policy
        )
        .is_err()
    );
    assert_eq!(evidence_name(SESSION, "r.json"), "d4cd9053a12d5c8e-r.json");
}
