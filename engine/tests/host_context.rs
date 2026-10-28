//! Native field semantics, identities and compaction transitions across host adapters.
use craft_engine::session::host_context::{decode, lifecycle, merge};
use serde_json::{Value, json};
const FIRST: &str = "2026-10-28T10:00:00.000Z";
const SECOND: &str = "2026-10-28T10:00:01.000Z";
const THIRD: &str = "2026-10-28T10:00:02.000Z";
fn codex() -> Value {
    json!({"method":"thread/tokenUsage/updated","params":{"threadId":"s1","turnId":"t1","tokenUsage":{
        "last":{"inputTokens":100,"cachedInputTokens":80,"outputTokens":10,"reasoningOutputTokens":5,"totalTokens":110},
        "total":{"inputTokens":10000,"outputTokens":2000,"totalTokens":12000},"modelContextWindow":1000}}})
}
fn compact(method: &str, id: &str) -> Value {
    json!({"method":method,"params":{"threadId":"s1","turnId":"t1","item":{"type":"contextCompaction","id":id}}})
}
#[test]
fn current_context_and_consumption_have_distinct_host_semantics() {
    let codex = decode("codex", &codex(), "s1", FIRST).unwrap();
    assert_eq!(
        codex["usage"]["currentTokens"], 100,
        "cached tokens already belong to inputTokens"
    );
    assert_eq!(codex["consumption"]["totalTokens"], 12000);
    let claude = json!({"session_id":"s1","model":{"id":"example"},"context_window":{"current_usage":{
        "input_tokens":10,"cache_creation_input_tokens":20,"cache_read_input_tokens":70,"output_tokens":50},"context_window_size":1000,
        "total_input_tokens":100,"total_output_tokens":50}});
    let observed = decode("claude", &claude, "s1", FIRST).unwrap();
    assert_eq!(observed["usage"]["currentTokens"], 100);
    assert!(
        observed.get("consumption").is_none(),
        "Claude window totals are not cumulative billing"
    );
    assert!(decode("claude", &claude, "another", FIRST).is_err());
    assert!(decode("codex", &codex, "another", FIRST).is_err());
}
#[test]
fn unavailable_and_invalid_counts_never_become_zero() {
    let mut event = codex();
    event["params"]["tokenUsage"]["last"]["inputTokens"] = Value::Null;
    assert!(decode("codex", &event, "s1", FIRST).is_err());
    event["params"]["tokenUsage"]["last"]["inputTokens"] = json!(-1);
    assert!(decode("codex", &event, "s1", FIRST).is_err());
    event["params"]["tokenUsage"]["last"]["inputTokens"] = json!(0);
    assert_eq!(
        decode("codex", &event, "s1", FIRST).unwrap()["usage"]["currentTokens"],
        0
    );
    assert!(decode("cursor", &event, "s1", FIRST).is_err());
    assert!(decode("codex", &event, "s1", "bad").is_err());
}
#[test]
fn compaction_invalidates_occupancy_and_requires_correlated_completion() {
    let usage = decode("codex", &codex(), "s1", FIRST).unwrap();
    let saved = merge(&Value::Null, &usage).unwrap();
    let start = decode("codex", &compact("item/started", "c1"), "s1", SECOND).unwrap();
    let started = merge(&saved, &start).unwrap();
    assert!(started.get("usage").is_none());
    assert_eq!(started["consumption"]["totalTokens"], 12000);
    let finish = decode("codex", &compact("item/completed", "c1"), "s1", THIRD).unwrap();
    assert_eq!(
        merge(&started, &finish).unwrap()["compaction"]["status"],
        "completed"
    );
    assert_eq!(
        merge(&Value::Null, &finish).unwrap()["compaction"]["status"],
        "completion-observed"
    );
    let wrong = decode("codex", &compact("item/completed", "other"), "s1", THIRD).unwrap();
    assert_eq!(
        merge(&started, &wrong).unwrap()["compaction"]["status"],
        "completion-observed"
    );
    assert!(
        merge(&started, &usage).is_err(),
        "out-of-order observation must not revive occupancy"
    );
    assert_eq!(
        merge(&started, &start).unwrap(),
        started,
        "duplicate receipt is idempotent"
    );
}
#[test]
fn hook_resume_is_observed_separately_from_completed_compaction() {
    let start = lifecycle("claude", "s1", "before-compact", FIRST).unwrap();
    let resume = lifecycle("claude", "s1", "after-compact", SECOND).unwrap();
    let next = merge(&merge(&Value::Null, &start).unwrap(), &resume).unwrap();
    assert_eq!(next["compaction"]["status"], "resume-observed");
    assert!(next.get("usage").is_none());
    let other = lifecycle("codex", "s1", "before-compact", THIRD).unwrap();
    assert!(merge(&next, &other).is_err());
}

#[test]
fn equal_timestamp_transitions_invalidate_but_cannot_restore_occupancy() {
    let usage = decode("codex", &codex(), "s1", FIRST).unwrap();
    let saved = merge(&Value::Null, &usage).unwrap();
    let start = decode("codex", &compact("item/started", "c1"), "s1", FIRST).unwrap();
    let started = merge(&saved, &start).unwrap();
    assert!(started.get("usage").is_none());
    assert!(merge(&started, &usage).is_err());
    let missing = craft_engine::session::host_context::unavailable("codex", "s1", FIRST).unwrap();
    assert!(merge(&saved, &missing).unwrap()["usage"].is_null());
}

#[test]
fn hook_phases_do_not_erase_app_server_correlation_or_completion() {
    let start = decode("codex", &compact("item/started", "c1"), "s1", FIRST).unwrap();
    let started = merge(&Value::Null, &start).unwrap();
    let hook = lifecycle("codex", "s1", "after-compact", SECOND).unwrap();
    let resumed = merge(&started, &hook).unwrap();
    assert_eq!(resumed["compaction"]["id"], "c1");
    assert_eq!(resumed["hookCompaction"]["status"], "resume-observed");
    let completed = decode("codex", &compact("item/completed", "c1"), "s1", THIRD).unwrap();
    let completed = merge(&resumed, &completed).unwrap();
    assert_eq!(completed["compaction"]["status"], "completed");
    let again = lifecycle("codex", "s1", "after-compact", "2026-10-28T10:00:03.000Z").unwrap();
    assert_eq!(
        merge(&completed, &again).unwrap()["compaction"]["status"],
        "completed"
    );
}
