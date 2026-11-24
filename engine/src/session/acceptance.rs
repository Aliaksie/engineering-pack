//! Engineering contracts, attributed acceptance evidence and their pure coverage.
//! Payload versions are independent of the package and session formats.
use crate::values::{line_text, sha256, truthy};
use crate::{canonical, digest, discovery::scope, values::js_blank};
use serde_json::{Value, json};

type Result<T = ()> = std::result::Result<T, String>;

fn fail<T>(reason: &str) -> Result<T> {
    Err(format!("acceptance:{reason}"))
}

/// `reason` with what failed: `acceptance:invalid-value: one of check, manual`.
fn failed<T>(reason: &str, detail: &str) -> Result<T> {
    Err(format!("acceptance:{reason}: {detail}"))
}

/// The stable reason of a validation message: its first word. What follows names the entry
/// and field that failed, `acceptance:invalid-value at context.sources[0].kind: one of …`,
/// and is prose, not a contract.
pub fn reason(message: &str) -> &str {
    let word = message
        .split_once(' ')
        .map_or(message, |(reason, _)| reason);
    word.strip_suffix(':').unwrap_or(word)
}

/// `result` with its failure located under `segment`, a field name or a `[index]`.
fn at<T>(segment: impl std::fmt::Display, result: Result<T>) -> Result<T> {
    result.map_err(|message| {
        if !message.starts_with("acceptance:") {
            return message;
        }
        // `reason[ at path][: detail]`: only the path, which has no spaces, follows ` at `;
        // the detail may quote ids and is never parsed.
        let segment = segment.to_string();
        let (head, detail) = match message.split_once(": ") {
            Some((head, detail)) => (head, Some(detail)),
            None => (message.as_str(), None),
        };
        let located = match head.split_once(" at ") {
            Some((reason, path)) => {
                let joint = if path.starts_with('[') { "" } else { "." };
                format!("{reason} at {segment}{joint}{path}")
            }
            None => format!("{head} at {segment}"),
        };
        match detail {
            Some(detail) => format!("{located}: {detail}"),
            None => located,
        }
    })
}

/// `check` of `value[name]`, its failure located at `name`.
fn field<'a>(value: &'a Value, name: &str, check: impl FnOnce(&'a Value) -> Result) -> Result {
    at(name, check(&value[name]))
}

fn object(value: &Value, fields: &[&str]) -> Result {
    let Some(map) = value.as_object() else {
        return failed("invalid-fields", "not an object");
    };
    match map.keys().find(|k| !fields.contains(&k.as_str())) {
        Some(unknown) => {
            let unknown: String = unknown
                .chars()
                .filter(|c| !c.is_control())
                .take(64)
                .collect();
            failed(
                "invalid-fields",
                &format!("no field `{unknown}`; fields are {}", fields.join(", ")),
            )
        }
        None => Ok(()),
    }
}

fn text(value: &Value, max: usize) -> Result {
    match value.as_str() {
        Some(s) if !js_blank(s) && line_text(s, max) => Ok(()),
        _ => fail("invalid-text"),
    }
}

fn choice(value: &Value, values: &[&str]) -> Result {
    if values.iter().any(|v| value == *v) {
        Ok(())
    } else {
        failed("invalid-value", &format!("one of {}", values.join(", ")))
    }
}

fn boolean(value: &Value) -> Result {
    if value.is_boolean() {
        Ok(())
    } else {
        failed("invalid-value", "true or false")
    }
}

/// Each entry of a list of at most `max`, its failure located at `[index]`.
fn list<'a>(value: &'a Value, max: usize, mut visit: impl FnMut(&'a Value) -> Result) -> Result {
    match value.as_array() {
        Some(items) if items.len() <= max => items
            .iter()
            .enumerate()
            .try_for_each(|(index, item)| at(format!("[{index}]"), visit(item))),
        Some(_) => failed("capacity", &format!("up to {max} entries")),
        None => failed("capacity", "not a list"),
    }
}

fn unique<'a>(values: impl Iterator<Item = &'a Value>) -> Result {
    let mut seen = Vec::new();
    for value in values {
        if seen.contains(&value) {
            return failed("duplicate-id", &value.to_string());
        }
        seen.push(value);
    }
    Ok(())
}

fn items(value: &Value) -> &[Value] {
    value.as_array().map_or(&[], Vec::as_slice)
}

fn ids(value: &Value) -> Result {
    list(value, 32, |entry| text(entry, 128))?;
    unique(items(value).iter())
}

fn scopes(value: &Value) -> Result {
    list(value, 32, |entry| {
        text(entry, 512)?;
        if scope(entry)? != entry.as_str().unwrap() {
            return fail("invalid-scope");
        }
        Ok(())
    })?;
    unique(items(value).iter())?;
    if items(value).is_empty() {
        return fail("missing-scope");
    }
    Ok(())
}

fn timestamp(value: &Value) -> Result {
    let valid = value.as_str().is_some_and(|s| {
        let b = s.as_bytes();
        let digits = |range: std::ops::Range<usize>| b[range].iter().all(u8::is_ascii_digit);
        let shape = (b.len() == 20 || (b.len() == 24 && b[19] == b'.' && digits(20..23)))
            && digits(0..4)
            && b[4] == b'-'
            && digits(5..7)
            && b[7] == b'-'
            && digits(8..10)
            && b[10] == b'T'
            && digits(11..13)
            && b[13] == b':'
            && digits(14..16)
            && b[16] == b':'
            && digits(17..19)
            && b[b.len() - 1] == b'Z';
        shape && crate::timestamp::parse(s).is_some()
    });
    if valid { Ok(()) } else { fail("invalid-time") }
}

fn hash(value: &Value) -> Result {
    if sha256(value) {
        Ok(())
    } else {
        fail("invalid-digest")
    }
}

fn version(value: &Value) -> Result {
    if value["formatVersion"].as_f64() == Some(1.0) {
        Ok(())
    } else {
        fail("unsupported-version")
    }
}

fn size(value: &Value) -> Result {
    if serde_json::to_vec(value)
        .expect("JSON values serialize")
        .len()
        > 32768
    {
        fail("capacity")
    } else {
        Ok(())
    }
}

fn references(values: &Value, known: &[&str]) -> Result {
    ids(values)?;
    if let Some(unknown) = items(values)
        .iter()
        .find(|id| !known.contains(&id.as_str().unwrap()))
    {
        return failed("unknown-reference", &format!("no entry {unknown}"));
    }
    Ok(())
}

fn string_ids(values: &Value) -> Vec<&str> {
    items(values)
        .iter()
        .filter_map(|entry| entry["id"].as_str())
        .collect()
}

/// `null`, or present and valid text; an absent field is invalid.
fn nullable(value: &Value, field: &str) -> Result {
    match value.get(field) {
        Some(Value::Null) => Ok(()),
        Some(text_value) => at(field, text(text_value, 512)),
        None => at(field, failed("invalid-text", "missing; null when unknown")),
    }
}

const STATUSES: &[&str] = &["accepted", "proposed", "superseded", "unknown"];

fn source(value: &Value) -> Result {
    object(
        value,
        &[
            "id",
            "kind",
            "reference",
            "identity",
            "observedAt",
            "scope",
            "provenance",
            "status",
        ],
    )?;
    field(value, "id", |v| text(v, 128))?;
    field(value, "reference", |v| text(v, 1024))?;
    field(value, "kind", |v| choice(v, SOURCE_KINDS))?;
    nullable(value, "identity")?;
    field(value, "observedAt", timestamp)?;
    field(value, "scope", scopes)?;
    field(value, "provenance", |v| {
        choice(v, &["declared", "observed", "inferred"])
    })?;
    field(value, "status", |v| choice(v, STATUSES))
}

/// What a context source is: the user's request, an instruction file, configuration, code, a
/// check, a decision record or something outside the repository.
pub const SOURCE_KINDS: &[&str] = &[
    "request",
    "instruction",
    "configuration",
    "code",
    "check",
    "adr",
    "external",
];

fn constraint(value: &Value, sources: &[&str], known: &[&str]) -> Result {
    object(
        value,
        &[
            "id",
            "description",
            "sourceIds",
            "authority",
            "status",
            "scope",
            "required",
            "conflicts",
        ],
    )?;
    field(value, "id", |v| text(v, 128))?;
    field(value, "description", |v| text(v, 512))?;
    field(value, "sourceIds", |v| references(v, sources))?;
    if items(&value["sourceIds"]).is_empty() {
        return at("sourceIds", failed("missing-basis", "names no source"));
    }
    field(value, "authority", |v| {
        choice(v, &["request", "project", "observation"])
    })?;
    field(value, "status", |v| choice(v, STATUSES))?;
    field(value, "scope", scopes)?;
    field(value, "required", boolean)?;
    field(value, "conflicts", |v| references(v, known))?;
    if items(&value["conflicts"]).contains(&value["id"]) {
        return at("conflicts", failed("invalid-conflict", "names itself"));
    }
    Ok(())
}

pub fn validate_context(value: &Value) -> Result {
    object(
        value,
        &[
            "formatVersion",
            "scope",
            "sources",
            "constraints",
            "checks",
            "questions",
            "coverage",
        ],
    )?;
    version(value)?;
    size(value)?;
    field(value, "scope", scopes)?;
    field(value, "sources", |v| list(v, 32, source))?;
    at(
        "sources",
        unique(items(&value["sources"]).iter().map(|e| &e["id"])),
    )?;
    let sources = string_ids(&value["sources"]);
    let known = string_ids(&value["constraints"]);
    field(value, "constraints", |v| {
        list(v, 32, |entry| constraint(entry, &sources, &known))
    })?;
    at(
        "constraints",
        unique(items(&value["constraints"]).iter().map(|e| &e["id"])),
    )?;
    field(value, "checks", |v| {
        list(v, 32, |entry| {
            object(entry, &["id", "description", "sourceIds", "scope"])?;
            field(entry, "id", |v| text(v, 128))?;
            field(entry, "description", |v| text(v, 512))?;
            field(entry, "sourceIds", |v| references(v, &sources))?;
            field(entry, "scope", scopes)
        })
    })?;
    at(
        "checks",
        unique(items(&value["checks"]).iter().map(|e| &e["id"])),
    )?;
    field(value, "questions", |v| {
        list(v, 32, |entry| text(entry, 512))
    })?;
    field(value, "coverage", |v| {
        choice(v, &["complete", "incomplete", "unknown"])
    })
}

pub fn validate_contract(value: &Value) -> Result {
    object(
        value,
        &[
            "formatVersion",
            "intent",
            "exclusions",
            "context",
            "criteria",
            "decisions",
            "derivation",
        ],
    )?;
    version(value)?;
    size(value)?;
    field(value, "intent", |v| text(v, 512))?;
    field(value, "exclusions", |v| {
        list(v, 32, |entry| text(entry, 512))
    })?;
    field(value, "context", validate_context)?;
    let sources = string_ids(&value["context"]["sources"]);
    let checks = string_ids(&value["context"]["checks"]);
    field(value, "criteria", |v| {
        list(v, 32, |entry| {
            object(
                entry,
                &[
                    "id",
                    "description",
                    "required",
                    "scope",
                    "sourceIds",
                    "checkIds",
                    "evidenceKind",
                ],
            )?;
            field(entry, "id", |v| text(v, 128))?;
            field(entry, "description", |v| text(v, 512))?;
            field(entry, "scope", scopes)?;
            field(entry, "required", boolean)?;
            field(entry, "sourceIds", |v| references(v, &sources))?;
            field(entry, "checkIds", |v| references(v, &checks))?;
            if items(&entry["sourceIds"]).is_empty() {
                return at("sourceIds", failed("missing-basis", "names no source"));
            }
            field(entry, "evidenceKind", |v| choice(v, &["check", "manual"]))
        })
    })?;
    at(
        "criteria",
        unique(items(&value["criteria"]).iter().map(|e| &e["id"])),
    )?;
    if items(&value["criteria"]).is_empty() {
        return at("criteria", failed("missing-criteria", "none declared"));
    }
    field(value, "decisions", |v| {
        list(v, 32, |entry| {
            object(entry, &["id", "description", "blocking", "resolved"])?;
            field(entry, "id", |v| text(v, 128))?;
            field(entry, "description", |v| text(v, 512))?;
            field(entry, "blocking", boolean)?;
            field(entry, "resolved", boolean)
        })
    })?;
    at(
        "decisions",
        unique(items(&value["decisions"]).iter().map(|e| &e["id"])),
    )?;
    field(value, "derivation", |derivation| {
        object(derivation, &["reporter", "method", "observedAt"])?;
        field(derivation, "reporter", |v| text(v, 512))?;
        field(derivation, "observedAt", timestamp)?;
        field(derivation, "method", |v| {
            choice(v, &["declared", "observed", "inferred"])
        })
    })
}

pub fn contract_digest(value: &Value) -> Result<String> {
    validate_contract(value)?;
    Ok(digest(&canonical(value)))
}

fn binding(value: &Value) -> Result {
    object(value, &["kind", "identity"])?;
    field(value, "kind", |v| choice(v, &["repository", "environment"]))?;
    nullable(value, "identity")
}

/// The ledger format: 1, or 2 where a record may carry the test counts of its run.
fn evidence_version(value: &Value) -> Result<u8> {
    match value["formatVersion"].as_f64() {
        Some(1.0) => Ok(1),
        Some(2.0) => Ok(2),
        _ => fail("unsupported-version"),
    }
}

/// A v2 record's tests: what the runner executed, failed and skipped, the runner when known,
/// and whether Craft observed the run or the reporter declared the counts.
fn tests(value: &Value) -> Result {
    object(
        value,
        &["runner", "executed", "failed", "skipped", "source"],
    )?;
    nullable(value, "runner")?;
    // A count the runner did not report is left out: unknown, never zero.
    for count in ["executed", "failed", "skipped"] {
        if value
            .get(count)
            .is_some_and(|n| n.as_u64().is_none_or(|n| n > 1_000_000_000))
        {
            return at(count, failed("invalid-value", "a count up to 1000000000"));
        }
    }
    field(value, "source", |v| choice(v, &["observed", "declared"]))
}

pub fn validate_evidence(value: &Value) -> Result {
    object(value, &["formatVersion", "records"])?;
    let format = evidence_version(value)?;
    size(value)?;
    field(value, "records", |v| {
        list(v, 64, |entry| {
            if let Some(found) = entry.get("tests") {
                if format < 2 {
                    return at(
                        "tests",
                        failed("invalid-fields", "test counts need ledger format 2"),
                    );
                }
                at("tests", tests(found))?;
            }
            object(
                entry,
                &[
                    "tests",
                    "id",
                    "criterionIds",
                    "scope",
                    "contract",
                    "binding",
                    "checkId",
                    "kind",
                    "artifact",
                    "observedAt",
                    "reporter",
                    "basis",
                    "support",
                    "execution",
                ],
            )?;
            field(entry, "id", |v| text(v, 128))?;
            field(entry, "criterionIds", ids)?;
            field(entry, "scope", scopes)?;
            field(entry, "contract", hash)?;
            field(entry, "binding", binding)?;
            if items(&entry["criterionIds"]).is_empty() {
                return at(
                    "criterionIds",
                    failed("missing-criteria", "names no criterion"),
                );
            }
            field(entry, "checkId", |v| text(v, 128))?;
            field(entry, "kind", |v| choice(v, &["check", "manual"]))?;
            field(entry, "artifact", |v| text(v, 1024))?;
            field(entry, "observedAt", timestamp)?;
            field(entry, "reporter", |v| text(v, 512))?;
            field(entry, "basis", |v| text(v, 512))?;
            field(entry, "support", |v| {
                choice(v, &["verified", "observed", "inferred", "unknown"])
            })?;
            field(entry, "execution", |v| {
                choice(v, &["passed", "failed", "not-run", "not-available"])
            })
        })
    })?;
    at(
        "records",
        unique(items(&value["records"]).iter().map(|e| &e["id"])),
    )
}

/// Pure applicability over supplied observations; this does not verify artifacts or prose.
pub fn coverage(
    contract: Option<&Value>,
    evidence: Option<&Value>,
    current: &Value,
    blockers: bool,
) -> Result<Value> {
    let Some(contract) = contract.filter(|c| truthy(*c)) else {
        return Ok(
            json!({"assurance": "declared", "coverage": "unknown", "complete": false, "criteria": []}),
        );
    };
    let evidence = evidence.unwrap_or(&Value::Null);
    validate_contract(contract)?;
    validate_evidence(evidence)?;
    let format = evidence_version(evidence)?;
    binding(current)?;
    let fingerprint = contract_digest(contract)?;
    let records: Vec<Value> = items(&evidence["records"])
        .iter()
        .map(|entry| {
            let freshness = if entry["contract"] != fingerprint.as_str() {
                "stale"
            } else if entry["binding"]["identity"].is_null() || current["identity"].is_null() {
                "unknown"
            } else if entry["binding"]["kind"] == current["kind"]
                && entry["binding"]["identity"] == current["identity"]
            {
                "current"
            } else {
                "stale"
            };
            let mut record = entry.clone();
            record["freshness"] = json!(freshness);
            record
        })
        .collect();
    let known: Vec<&Value> = items(&contract["criteria"])
        .iter()
        .map(|c| &c["id"])
        .collect();
    for (index, record) in records.iter().enumerate() {
        if let Some(unknown) = items(&record["criterionIds"])
            .iter()
            .find(|id| !known.contains(id))
        {
            return at(
                format!("records[{index}].criterionIds"),
                failed("unknown-reference", &format!("no criterion {unknown}")),
            );
        }
    }
    let criteria: Vec<Value> = items(&contract["criteria"])
        .iter()
        .map(|criterion| {
            let applicable: Vec<&Value> = records
                .iter()
                .filter(|entry| {
                    items(&entry["criterionIds"]).contains(&criterion["id"])
                        && entry["freshness"] == "current"
                        && entry["kind"] == criterion["evidenceKind"]
                        && items(&criterion["checkIds"]).contains(&entry["checkId"])
                        && items(&criterion["scope"])
                            .iter()
                            .all(|scope| items(&entry["scope"]).contains(scope))
                })
                .collect();
            let ran = |e: &&&Value| e["execution"] == "passed" || e["execution"] == "failed";
            // A run that executed no test, or counted failures, failed whatever it declares.
            let refuted = |e: &&&Value| {
                ran(e)
                    && e.get("tests").is_some_and(|t| {
                        t["executed"] == 0 || t["failed"].as_u64().is_some_and(|n| n > 0)
                    })
            };
            let failed = applicable
                .iter()
                .any(|e| e["execution"] == "failed" || refuted(&e));
            // A v2 run passes only with a runner that reported executed tests.
            let verified = !failed
                && applicable.iter().any(|e| {
                    e["support"] == "verified"
                        && e["execution"] == "passed"
                        && (format < 2 || e.get("tests").is_none_or(|t| !t["runner"].is_null()))
                });
            let status = if failed {
                "failed"
            } else if verified {
                "verified"
            } else if format >= 2 && !applicable.is_empty() {
                // Current evidence that does not decide: unverified, not run, unknown runner.
                "unknown"
            } else {
                "gap"
            };
            let mut entry = json!({
                "id": criterion["id"],
                "required": criterion["required"],
                "status": status,
                "evidenceIds": applicable.iter().map(|e| e["id"].clone()).collect::<Vec<_>>(),
            });
            // Every failing run reported that it executed no test: nothing was checked, rather
            // than a check failing. A run without counts says nothing about how many ran.
            let failing: Vec<&&Value> = applicable
                .iter()
                .filter(|e| e["execution"] == "failed" || refuted(e))
                .collect();
            if !failing.is_empty() && failing.iter().all(|e| e["tests"]["executed"] == 0) {
                entry["reason"] = json!("zero-tests");
            }
            entry
        })
        .collect();
    let conflicts: Vec<Value> = items(&contract["context"]["constraints"])
        .iter()
        .filter(|c| {
            c["required"] == true
                && c["status"] != "superseded"
                && (c["status"] != "accepted" || !items(&c["conflicts"]).is_empty())
        })
        .map(|c| c["id"].clone())
        .collect();
    let decisions: Vec<Value> = items(&contract["decisions"])
        .iter()
        .filter(|d| d["blocking"] == true && d["resolved"] != true)
        .map(|d| d["id"].clone())
        .collect();
    let complete = !blockers
        && conflicts.is_empty()
        && decisions.is_empty()
        && criteria
            .iter()
            .all(|c| c["required"] != true || c["status"] == "verified");
    Ok(json!({
        "assurance": "attributed",
        "contract": fingerprint,
        "criteria": criteria,
        "records": records,
        "conflicts": conflicts,
        "decisions": decisions,
        "complete": complete,
    }))
}
