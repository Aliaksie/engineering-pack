//! A structural lookup: refresh Project Intelligence within a short budget, answer from verified
//! evidence for the sources it covers and search the remaining in-scope sources literally, or
//! the whole scope when the index cannot answer.
use crate::{budget::Budget, failure::Failure, project};
use craft_engine::{
    contracts,
    discovery::question::{self, Coverage, Question},
};
use craft_platform::{
    providers::{ACTIVE, for_path, kinds},
    repository::database_guard,
    state,
    store::{Query, Store},
};
use serde_json::{Value, json};
use std::path::Path;

/// Extraction time one routed lookup may spend; sources it defers stay pending and are
/// searched literally, and a later lookup continues the refresh.
const REFRESH_MS: u64 = 2_000;

/// Answer `input` (`ask`, `value`, `paths`, optional `limit` and `after`). `locate` runs a
/// literal search request. `route.complete` says whether an empty answer is an absence.
pub fn answer(
    root: &Path,
    input: &Value,
    locate: impl FnOnce(&Value) -> Result<Value, Failure>,
) -> Result<Value, Failure> {
    let question = question::question(input)?;
    let root = state::canonical_root(root)?;
    let mut coverage = Coverage {
        enabled: contracts::evidence_enabled(&project::configuration(&root)?)?,
        ..Coverage::default()
    };
    let mut answer = None;
    if coverage.enabled {
        match observe(&root, &question, &mut coverage) {
            Ok(result) => answer = result,
            Err(failure) => coverage.unavailable = Some(failure.message),
        }
    }
    let mut route = question::route(&question, &coverage);
    let (mut result, complete) = match (route["answeredBy"].as_str(), answer) {
        (Some("intelligence"), Some(answer)) => (answer, true),
        // A continued page carries only further index rows; the first page searched the gaps.
        (Some("intelligence+locate"), Some(answer)) if !question.after.is_empty() => {
            (answer, false)
        }
        (Some("intelligence+locate"), Some(mut answer)) => {
            let gaps = locate(&question::literal(&question, Some(&coverage.gaps())))?;
            let complete = gaps["truncated"] == false;
            answer["gapSearch"] = gaps;
            (answer, complete)
        }
        _ => (locate(&question::literal(&question, None))?, false),
    };
    route["complete"] = json!(complete);
    result["route"] = route;
    Ok(result)
}

/// Refresh and record the scope's coverage; when the index can answer, query it and verify the
/// cited sources, leaving out rows from sources that changed since the refresh.
fn observe(
    root: &Path,
    question: &Question,
    coverage: &mut Coverage,
) -> Result<Option<Value>, Failure> {
    let database = state::shared(root, "intelligence", true)?.join("project.sqlite");
    let _guard = database_guard(&database)?;
    let mut store = Store::open(&database)?;
    let (field, wanted) = question.ask.evidence();
    let answering: Vec<_> = ACTIVE
        .iter()
        .copied()
        .filter(|provider| kinds(provider).iter().any(|kind| wanted.contains(kind)))
        .collect();
    let mut budget = Budget::from_input(&json!({"maxDurationMs": REFRESH_MS}))?;
    let refreshed = crate::evidence::refresh(root, &mut store, &mut budget, &mut |path| {
        if question::within(path, &question.scopes)
            && question.ask.candidate(path)
            && !for_path(path).is_some_and(|provider| answering.contains(&provider))
        {
            coverage.unindexed.push(path.to_string());
        }
    })?;
    let status = &refreshed["coverage"];
    coverage.inventory_complete = status["inventoryComplete"] == true;
    let exclusions: Vec<String> = serde_json::from_value(status["exclusions"].clone())
        .map_err(|_| "invalid stored evidence exclusions")?;
    coverage.excluded = question
        .scopes
        .iter()
        .filter(|scope| contracts::excluded(scope, &exclusions))
        .cloned()
        .collect();
    for (path, provider, outcome) in store.unextracted()? {
        if answering.contains(&provider.as_str()) && question::within(&path, &question.scopes) {
            match outcome.as_str() {
                "failed" => coverage.failed.push(path),
                _ => coverage.pending.push(path),
            }
        }
    }
    if question::route(question, coverage)["answeredBy"] == "locate" {
        return Ok(None);
    }
    let under: Vec<&str> = question
        .scopes
        .iter()
        .filter(|scope| *scope != ".")
        .map(String::as_str)
        .collect();
    let mut result = store.query(&Query {
        field,
        value: &question.value,
        after: &question.after,
        limit: question.limit,
        source: None,
        kinds: wanted,
        // An outline's document is its source, matched by the path field itself.
        under: if field == "path" { &[] } else { &under },
    })?;
    crate::evidence::verify(root, &mut result, true)?;
    let sources = result["sources"]
        .as_object_mut()
        .ok_or("invalid query sources")?;
    sources.retain(|path, source| {
        let current = source["verification"] == "verified-selected-source";
        if !current {
            coverage.changed.push(path.clone());
        }
        current
    });
    if !coverage.changed.is_empty() {
        let changed = &coverage.changed;
        result["results"]
            .as_array_mut()
            .ok_or("invalid query results")?
            .retain(|row| !changed.iter().any(|path| row["path"] == path.as_str()));
    }
    result["refresh"] = json!({
        "changed": refreshed["changed"], "deferred": refreshed["deferred"],
        "wallTimeMs": refreshed["wallTimeMs"]});
    Ok(Some(result))
}

/// Text view: the route, the index rows grouped by source, then literal matches.
pub fn text(result: &Value) -> String {
    let shown = crate::render::shown;
    let route = &result["route"];
    let mut text = format!(
        "{} ({}): {} {} in {}",
        shown(&route["answeredBy"]),
        shown(&route["reason"]),
        shown(&route["question"]),
        shown(&route["value"]),
        route["scopes"]
            .as_array()
            .map(|scopes| scopes.iter().map(shown).collect::<Vec<_>>().join(", "))
            .unwrap_or_default()
    );
    match &route["detail"] {
        Value::Object(detail) => {
            let gaps: Vec<_> = detail
                .iter()
                .map(|(name, value)| match value.get("count") {
                    Some(count) => format!("{name} {}", shown(count)),
                    None => format!(
                        "{name} {}",
                        value
                            .as_object()
                            .map(|counts| counts
                                .iter()
                                .map(|(ext, n)| format!("{ext} {}", shown(n)))
                                .collect::<Vec<_>>()
                                .join(" "))
                            .unwrap_or_default()
                    ),
                })
                .collect();
            text.push_str(&format!("; gaps: {}", gaps.join(", ")));
        }
        Value::Null => {}
        detail => text.push_str(&format!("; {}", shown(detail))),
    }
    text.push('\n');
    let literal = |text: &mut String, search: &Value, label: &str| {
        let matches = search["matches"].as_array().map_or(&[][..], Vec::as_slice);
        text.push_str(&format!(
            "{label}: {} match(es){}\n",
            matches.len(),
            if search["truncated"] == true {
                format!(", truncated by {}", shown(&search["truncation"]))
            } else {
                String::new()
            }
        ));
        for found in matches {
            text.push_str(&format!(
                "  {}:{}: {}\n",
                shown(&found["path"]),
                shown(&found["line"]),
                shown(&found["text"])
            ));
        }
    };
    let rows = result["results"].as_array();
    if rows.is_some() {
        text.push_str(&crate::evidence::render(result, false));
    }
    if result["gapSearch"].is_object() {
        literal(
            &mut text,
            &result["gapSearch"],
            "literal search of gap files",
        );
    } else if rows.is_none() {
        literal(&mut text, result, "literal search");
    }
    let any = |list: &Value| list.as_array().is_some_and(|list| !list.is_empty());
    if !any(&result["results"]) && !any(&result["gapSearch"]["matches"]) && !any(&result["matches"])
    {
        text.push_str(if route["complete"] == true {
            "No match: absent within this coverage.\n"
        } else {
            "No match: not proof of absence.\n"
        });
    }
    text
}
