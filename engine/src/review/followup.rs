//! Reviewer follow-up: what a reviewer should do next on a pull request, derived only from
//! a GitHub GraphQL observation and explicit facts. Nothing is stored between checks; a
//! truncated observation is reported rather than read as a quiet review.
use crate::contracts::js_blank;
use crate::timestamp;
use serde_json::{Map, Value, json};
use std::collections::HashSet;

const DAY_MS: i64 = 86_400_000;
const SUMMARY_CHARS: usize = 120;

/// The reviewer and the moment a check is made for.
pub struct Facts<'a> {
    pub reviewer: &'a str,
    pub now: i64,
    pub stale_days: i64,
    /// Resolved threads the reviewer has already verified.
    pub acknowledged: &'a HashSet<String>,
}

struct Event<'a> {
    author: &'a str,
    time: i64,
    substantive: bool,
}

fn nodes<'a>(connection: &'a Value, label: &str) -> Result<&'a [Value], String> {
    let (Some(items), Some(more)) = (
        connection["nodes"].as_array(),
        connection["pageInfo"]["hasNextPage"].as_bool(),
    ) else {
        return Err(format!("missing {label} connection"));
    };
    if more {
        return Err(format!("{label} truncated; fetch the rest before deciding"));
    }
    Ok(items)
}

fn instant(value: &Value, label: &str) -> Result<i64, String> {
    value
        .as_str()
        .and_then(timestamp::parse)
        .ok_or_else(|| format!("invalid {label} time"))
}

/// `Date.prototype.toISOString` without a zero fraction, as GitHub writes times.
fn written(ms: i64) -> String {
    let text = timestamp::format(ms);
    text.strip_suffix(".000Z")
        .map_or(text.clone(), |prefix| format!("{prefix}Z"))
}

/// A deleted account has no author; its contributions count for nobody.
fn login(item: &Value) -> Option<&str> {
    item["author"]["login"]
        .as_str()
        .filter(|login| !login.is_empty())
}

fn bot(item: &Value) -> bool {
    item["author"]["__typename"] == "Bot"
}

fn first_line(body: &Value) -> String {
    let body = body.as_str().unwrap_or_default();
    let line = body
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .find(|line| !js_blank(line))
        .unwrap_or_default();
    line.chars().take(SUMMARY_CHARS).collect()
}

/// The follow-up for one pull request node, or why its observation cannot support one.
pub fn follow_up(pull: &Value, facts: &Facts) -> Result<Value, String> {
    let me = facts.reviewer;
    let head = pull["headRefOid"]
        .as_str()
        .filter(|head| !head.is_empty())
        .ok_or("missing head revision")?;
    // Only the newest commit is needed, so earlier commit pages never make this incomplete.
    let commit = &pull["commits"]["nodes"]
        .as_array()
        .and_then(|commits| commits.last())
        .ok_or("missing head commit")?["commit"];
    if commit["oid"] != head {
        return Err("head commit does not match the head revision".into());
    }
    let committed = instant(&commit["committedDate"], "commit")?;
    let reviews = nodes(&pull["reviews"], "reviews")?;
    let comments = nodes(&pull["comments"], "comments")?;
    let threads = nodes(&pull["reviewThreads"], "review threads")?;

    let mut events = Vec::new();
    let mut approval = 0;
    let mut reviewed: Option<(i64, &str)> = None;
    let (mut approved_by_other, mut changes_requested) = (false, false);
    for review in reviews {
        // A pending review has no submission time and nobody else can see it.
        if review["submittedAt"].is_null() {
            continue;
        }
        let time = instant(&review["submittedAt"], "review")?;
        let Some(author) = login(review) else {
            continue;
        };
        let state = review["state"].as_str().unwrap_or_default();
        let body = review["body"].as_str().is_some_and(|body| !js_blank(body));
        events.push(Event {
            author,
            time,
            substantive: (state != "APPROVED" || body) && !bot(review),
        });
        if author == me {
            if let Some(oid) = review["commit"]["oid"].as_str()
                && reviewed.is_none_or(|(at, _)| time >= at)
            {
                reviewed = Some((time, oid));
            }
        } else if state == "APPROVED" {
            approved_by_other = true;
            approval = approval.max(time);
        } else if state == "CHANGES_REQUESTED" {
            changes_requested = true;
        }
    }
    for comment in comments {
        let time = instant(&comment["createdAt"], "comment")?;
        if let Some(author) = login(comment) {
            events.push(Event {
                author,
                time,
                substantive: !bot(comment),
            });
        }
    }
    let mut participating = Vec::new();
    for thread in threads {
        let comments = nodes(&thread["comments"], "thread comments")?;
        let mut newest: Option<(i64, &Value)> = None;
        for comment in comments {
            let time = instant(&comment["createdAt"], "thread comment")?;
            if let Some(author) = login(comment) {
                events.push(Event {
                    author,
                    time,
                    substantive: !bot(comment),
                });
            }
            if newest.is_none_or(|(at, _)| time > at) {
                newest = Some((time, comment));
            }
        }
        if !comments.iter().any(|comment| login(comment) == Some(me)) {
            continue;
        }
        let last = newest.map(|(_, comment)| comment);
        let last_author = last.and_then(login);
        participating.push(json!({
            "id": thread["id"],
            "path": thread["path"],
            "line": thread["line"],
            "isResolved": thread["isResolved"],
            "isOutdated": thread["isOutdated"],
            "lastCommentAuthor": last_author,
            "lastCommentAt": last.map_or(&Value::Null, |comment| &comment["createdAt"]),
            "repliedByOther": last_author.is_some_and(|author| author != me),
            "summary": first_line(&comments[0]["body"]),
        }));
    }

    let latest = |keep: &dyn Fn(&Event) -> bool| {
        events
            .iter()
            .filter(|event| keep(event))
            .map(|event| event.time)
            .max()
    };
    let mine = latest(&|event| event.author == me);
    let other = latest(&|event| event.author != me);
    let reply = latest(&|event| event.author != me && event.substantive).unwrap_or(0);
    let open = participating
        .iter()
        .filter(|thread| thread["isResolved"] == false)
        .count();
    let resolved: Vec<&Value> = participating
        .iter()
        .filter(|thread| thread["isResolved"] == true)
        .map(|thread| &thread["id"])
        .filter(|id| {
            !id.as_str()
                .is_some_and(|id| facts.acknowledged.contains(id))
        })
        .collect();
    let reviewed_head = reviewed.map(|(_, oid)| oid);
    let head_moved = reviewed_head.is_some_and(|oid| oid != head);
    let age = mine.map(|mine| (facts.now - mine).div_euclid(DAY_MS));
    let state = pull["state"].as_str();
    let status = match mine {
        _ if state.is_some_and(|state| state != "OPEN") => "closed",
        None => "new",
        Some(mine) if committed > mine || head_moved || !resolved.is_empty() => {
            "fixed-pending-verify"
        }
        Some(mine) if reply > mine => "answered",
        Some(mine) if approval > mine && open == 0 => "clean",
        _ if age.is_some_and(|age| age > facts.stale_days) => "stale",
        _ => "awaiting-author",
    };
    let after = |time: i64| mine.is_some_and(|mine| time > mine);
    let mut result = Map::new();
    for name in ["number", "url", "title", "state", "isDraft"] {
        result.insert(name.into(), pull[name].clone());
    }
    result.insert("author".into(), json!(login(pull)));
    result.insert("ownPullRequest".into(), json!(login(pull) == Some(me)));
    result.insert("head".into(), json!(head));
    result.insert("status".into(), json!(status));
    result.insert(
        "signals".into(),
        json!({
            "reviewerLastAt": mine.map(written),
            "othersLastAt": other.map(written),
            "daysSinceReviewer": age,
            "replyAfterReviewer": after(reply),
            "approvedAfterReviewer": after(approval),
            "commitAfterReviewer": after(committed),
            "reviewedHead": reviewed_head,
            "headMovedSinceReview": head_moved,
            "openThreads": open,
            "repliedThreads": participating.iter().filter(|thread| thread["repliedByOther"] == true).count(),
            "resolvedThreads": resolved,
            "approvedByOther": approved_by_other,
            "changesRequestedByOther": changes_requested,
        }),
    );
    result.insert("threads".into(), Value::Array(participating));
    Ok(Value::Object(result))
}

/// Follow-ups for every pull request in a `gh api graphql` response that selected either
/// `repository.pullRequest` or `repository.pullRequests`.
pub fn check(response: &Value, facts: &Facts) -> Result<Value, String> {
    if response["errors"]
        .as_array()
        .is_some_and(|errors| !errors.is_empty())
    {
        return Err("the GraphQL response reports errors".into());
    }
    let repository = &response["data"]["repository"];
    let (pulls, listed_all) = if repository["pullRequest"].is_object() {
        (std::slice::from_ref(&repository["pullRequest"]), true)
    } else if let Some(pulls) = repository["pullRequests"]["nodes"].as_array() {
        let more = repository["pullRequests"]["pageInfo"]["hasNextPage"].as_bool();
        (pulls.as_slice(), more == Some(false))
    } else {
        return Err("expected data.repository.pullRequest or data.repository.pullRequests".into());
    };
    let mut seen = HashSet::new();
    let mut complete = listed_all;
    let mut results = Vec::new();
    for pull in pulls {
        if !pull.is_object() || !seen.insert(pull["number"].to_string()) {
            return Err("pull requests must be distinct objects".into());
        }
        results.push(follow_up(pull, facts).unwrap_or_else(|reason| {
            complete = false;
            json!({"number": pull["number"], "url": pull["url"], "status": null, "incomplete": reason})
        }));
    }
    Ok(json!({
        "reviewer": facts.reviewer,
        "now": written(facts.now),
        "complete": complete,
        "pullRequests": results,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: &str = "auditor";
    const HEAD: &str = "1111111111111111111111111111111111111111";

    fn page(nodes: Value) -> Value {
        json!({"pageInfo": {"hasNextPage": false}, "nodes": nodes})
    }

    fn pull(number: i64, threads: Value) -> Value {
        json!({
            "number": number, "state": "OPEN", "headRefOid": HEAD, "author": {"login": "developer"},
            "commits": {"nodes": [{"commit": {"oid": HEAD, "committedDate": "2026-08-01T00:00:00Z"}}]},
            "comments": page(json!([{"author": {"login": ME}, "createdAt": "2026-08-10T00:00:00Z", "body": "Why?"}])),
            "reviews": page(json!([])),
            "reviewThreads": page(threads),
        })
    }

    fn resolved(id: &str) -> Value {
        json!({"id": id, "isResolved": true, "comments": page(json!([
            {"author": {"login": ME}, "createdAt": "2026-08-10T00:00:00Z", "body": "Bound it."}
        ]))})
    }

    fn facts(acknowledged: &HashSet<String>) -> Facts<'_> {
        Facts {
            reviewer: ME,
            now: timestamp::parse("2026-08-12T00:00:00Z").unwrap(),
            stale_days: 5,
            acknowledged,
        }
    }

    #[test]
    fn acknowledged_resolutions_stop_asking_for_verification() {
        let threads = json!([resolved("T1"), resolved("T2")]);
        let none = HashSet::new();
        let result = follow_up(&pull(1, threads.clone()), &facts(&none)).unwrap();
        assert_eq!(result["status"], "fixed-pending-verify");
        assert_eq!(result["signals"]["resolvedThreads"], json!(["T1", "T2"]));
        let one = HashSet::from(["T1".to_string()]);
        let result = follow_up(&pull(1, threads.clone()), &facts(&one)).unwrap();
        assert_eq!(result["signals"]["resolvedThreads"], json!(["T2"]));
        let both = HashSet::from(["T1".to_string(), "T2".to_string()]);
        let result = follow_up(&pull(1, threads), &facts(&both)).unwrap();
        assert_eq!(result["status"], "awaiting-author");
    }

    #[test]
    fn the_reviewed_head_comes_from_the_latest_own_review() {
        let mut observation = pull(1, json!([]));
        observation["reviews"] = page(json!([
            {"author": {"login": ME}, "state": "COMMENTED", "submittedAt": "2026-08-09T00:00:00Z", "commit": {"oid": "old"}},
            {"author": {"login": ME}, "state": "COMMENTED", "submittedAt": "2026-08-10T00:00:00Z", "commit": {"oid": HEAD}},
        ]));
        let none = HashSet::new();
        let result = follow_up(&observation, &facts(&none)).unwrap();
        assert_eq!(result["signals"]["reviewedHead"], HEAD);
        assert_eq!(result["signals"]["headMovedSinceReview"], false);
        assert_eq!(result["status"], "awaiting-author");
    }

    #[test]
    fn a_list_reports_each_pull_request_and_its_completeness() {
        let none = HashSet::new();
        let mut broken = pull(2, json!([]));
        broken["comments"]["pageInfo"]["hasNextPage"] = json!(true);
        let list = |more: bool, nodes: Value| json!({"data": {"repository": {"pullRequests": {"pageInfo": {"hasNextPage": more}, "nodes": nodes}}}});
        let result = check(
            &list(false, json!([pull(1, json!([])), broken])),
            &facts(&none),
        )
        .unwrap();
        assert_eq!(result["complete"], false);
        assert_eq!(result["pullRequests"][0]["status"], "awaiting-author");
        assert_eq!(result["pullRequests"][1]["number"], 2);
        assert_eq!(result["pullRequests"][1]["status"], Value::Null);
        let result = check(&list(true, json!([pull(1, json!([]))])), &facts(&none)).unwrap();
        assert_eq!(
            result["complete"], false,
            "more pull requests remain unlisted"
        );
        let mut unpaged = list(false, json!([pull(1, json!([]))]));
        unpaged["data"]["repository"]["pullRequests"]
            .as_object_mut()
            .unwrap()
            .remove("pageInfo");
        let result = check(&unpaged, &facts(&none)).unwrap();
        assert_eq!(result["complete"], false, "an unpaged list may be partial");
        let result = check(&list(false, json!([pull(1, json!([]))])), &facts(&none)).unwrap();
        assert_eq!(result["complete"], true);
        assert!(
            check(
                &list(false, json!([pull(1, json!([])), pull(1, json!([]))])),
                &facts(&none)
            )
            .is_err()
        );
        assert!(check(&list(false, json!([1])), &facts(&none)).is_err());
    }

    #[test]
    fn unusable_responses_are_refused() {
        let none = HashSet::new();
        for response in [
            json!({"errors": [{"message": "Could not resolve"}], "data": {"repository": {"pullRequest": pull(1, json!([]))}}}),
            json!({"data": {"repository": null}}),
            json!({"data": {"repository": {"pullRequest": null}}}),
            json!([]),
        ] {
            assert!(check(&response, &facts(&none)).is_err(), "{response}");
        }
    }
}
