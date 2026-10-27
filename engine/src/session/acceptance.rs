//! Engineering contracts, attributed acceptance evidence and their pure coverage.
//! Payload versions are independent of the package and session formats.
use crate::values::{line_text, sha256, truthy};
use crate::{canonical, digest, discovery::scope, values::js_blank};
use serde_json::{Value, json};

type Result<T = ()> = std::result::Result<T, String>;

fn fail<T>(reason: &str) -> Result<T> {
    Err(format!("acceptance:{reason}"))
}

fn object(value: &Value, fields: &[&str]) -> Result {
    match value.as_object() {
        Some(map) if map.keys().all(|k| fields.contains(&k.as_str())) => Ok(()),
        _ => fail("invalid-fields"),
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
        fail("invalid-value")
    }
}

fn list<'a>(value: &'a Value, max: usize, mut visit: impl FnMut(&'a Value) -> Result) -> Result {
    match value.as_array() {
        Some(items) if items.len() <= max => items.iter().try_for_each(&mut visit),
        _ => fail("capacity"),
    }
}

fn unique<'a>(values: impl Iterator<Item = &'a Value>) -> Result {
    let mut seen = Vec::new();
    for value in values {
        if seen.contains(&value) {
            return fail("duplicate-id");
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
    if items(values)
        .iter()
        .any(|id| !known.contains(&id.as_str().unwrap()))
    {
        return fail("unknown-reference");
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
        Some(text_value) => text(text_value, 512),
        None => fail("invalid-text"),
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
    text(&value["id"], 128)?;
    text(&value["reference"], 1024)?;
    choice(
        &value["kind"],
        &[
            "request",
            "instruction",
            "configuration",
            "code",
            "check",
            "adr",
            "external",
        ],
    )?;
    nullable(value, "identity")?;
    timestamp(&value["observedAt"])?;
    scopes(&value["scope"])?;
    choice(&value["provenance"], &["declared", "observed", "inferred"])?;
    choice(&value["status"], STATUSES)
}

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
    text(&value["id"], 128)?;
    text(&value["description"], 512)?;
    references(&value["sourceIds"], sources)?;
    if items(&value["sourceIds"]).is_empty() {
        return fail("missing-basis");
    }
    choice(&value["authority"], &["request", "project", "observation"])?;
    choice(&value["status"], STATUSES)?;
    scopes(&value["scope"])?;
    if !value["required"].is_boolean() {
        return fail("invalid-value");
    }
    references(&value["conflicts"], known)?;
    if items(&value["conflicts"]).contains(&value["id"]) {
        return fail("invalid-conflict");
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
    scopes(&value["scope"])?;
    list(&value["sources"], 32, source)?;
    unique(items(&value["sources"]).iter().map(|e| &e["id"]))?;
    let sources = string_ids(&value["sources"]);
    let known = string_ids(&value["constraints"]);
    list(&value["constraints"], 32, |entry| {
        constraint(entry, &sources, &known)
    })?;
    unique(items(&value["constraints"]).iter().map(|e| &e["id"]))?;
    list(&value["checks"], 32, |entry| {
        object(entry, &["id", "description", "sourceIds", "scope"])?;
        text(&entry["id"], 128)?;
        text(&entry["description"], 512)?;
        references(&entry["sourceIds"], &sources)?;
        scopes(&entry["scope"])
    })?;
    unique(items(&value["checks"]).iter().map(|e| &e["id"]))?;
    list(&value["questions"], 32, |entry| text(entry, 512))?;
    choice(&value["coverage"], &["complete", "incomplete", "unknown"])
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
    text(&value["intent"], 512)?;
    list(&value["exclusions"], 32, |entry| text(entry, 512))?;
    validate_context(&value["context"])?;
    let sources = string_ids(&value["context"]["sources"]);
    let checks = string_ids(&value["context"]["checks"]);
    list(&value["criteria"], 32, |entry| {
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
        text(&entry["id"], 128)?;
        text(&entry["description"], 512)?;
        scopes(&entry["scope"])?;
        if !entry["required"].is_boolean() {
            return fail("invalid-value");
        }
        references(&entry["sourceIds"], &sources)?;
        references(&entry["checkIds"], &checks)?;
        if items(&entry["sourceIds"]).is_empty() {
            return fail("missing-basis");
        }
        choice(&entry["evidenceKind"], &["check", "manual"])
    })?;
    unique(items(&value["criteria"]).iter().map(|e| &e["id"]))?;
    if items(&value["criteria"]).is_empty() {
        return fail("missing-criteria");
    }
    list(&value["decisions"], 32, |entry| {
        object(entry, &["id", "description", "blocking", "resolved"])?;
        text(&entry["id"], 128)?;
        text(&entry["description"], 512)?;
        if !entry["blocking"].is_boolean() || !entry["resolved"].is_boolean() {
            return fail("invalid-value");
        }
        Ok(())
    })?;
    unique(items(&value["decisions"]).iter().map(|e| &e["id"]))?;
    let derivation = &value["derivation"];
    object(derivation, &["reporter", "method", "observedAt"])?;
    text(&derivation["reporter"], 512)?;
    timestamp(&derivation["observedAt"])?;
    choice(&derivation["method"], &["declared", "observed", "inferred"])
}

pub fn contract_digest(value: &Value) -> Result<String> {
    validate_contract(value)?;
    Ok(digest(&canonical(value)))
}

fn binding(value: &Value) -> Result {
    object(value, &["kind", "identity"])?;
    choice(&value["kind"], &["repository", "environment"])?;
    nullable(value, "identity")
}

pub fn validate_evidence(value: &Value) -> Result {
    object(value, &["formatVersion", "records"])?;
    version(value)?;
    size(value)?;
    list(&value["records"], 64, |entry| {
        object(
            entry,
            &[
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
        text(&entry["id"], 128)?;
        ids(&entry["criterionIds"])?;
        scopes(&entry["scope"])?;
        hash(&entry["contract"])?;
        binding(&entry["binding"])?;
        if items(&entry["criterionIds"]).is_empty() {
            return fail("missing-criteria");
        }
        text(&entry["checkId"], 128)?;
        choice(&entry["kind"], &["check", "manual"])?;
        text(&entry["artifact"], 1024)?;
        timestamp(&entry["observedAt"])?;
        text(&entry["reporter"], 512)?;
        text(&entry["basis"], 512)?;
        choice(
            &entry["support"],
            &["verified", "observed", "inferred", "unknown"],
        )?;
        choice(
            &entry["execution"],
            &["passed", "failed", "not-run", "not-available"],
        )
    })?;
    unique(items(&value["records"]).iter().map(|e| &e["id"]))
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
    if records.iter().any(|r| {
        items(&r["criterionIds"])
            .iter()
            .any(|id| !known.contains(&id))
    }) {
        return fail("unknown-reference");
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
            let failed = applicable.iter().any(|e| e["execution"] == "failed");
            let verified = !failed
                && applicable
                    .iter()
                    .any(|e| e["support"] == "verified" && e["execution"] == "passed");
            json!({
                "id": criterion["id"],
                "required": criterion["required"],
                "status": if failed { "failed" } else if verified { "verified" } else { "gap" },
                "evidenceIds": applicable.iter().map(|e| e["id"].clone()).collect::<Vec<_>>(),
            })
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
