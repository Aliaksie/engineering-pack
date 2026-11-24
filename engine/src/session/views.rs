//! Reader views over one engineering contract and its evidence ledger: QA probes, product
//! gaps and architecture constraints. Each view reuses the acceptance coverage and only
//! selects or links supplied facts; none adds an observation or persists state.
use super::acceptance::coverage;
use crate::values::{js_blank, line_text, truthy};
use serde_json::{Map, Value, json};
use std::collections::HashSet;

type Result<T = ()> = std::result::Result<T, String>;

/// Probe detail is transient view input, bounded like the contract it explores.
const PROBES: usize = 32;
const PROBE_BYTES: usize = 32 * 1024;
const TEXT: usize = 1024;
const PROBE_FIELDS: [&str; 8] = [
    "id",
    "criterionIds",
    "risk",
    "flows",
    "preconditions",
    "actions",
    "expectedBasis",
    "evidenceIds",
];
const FOCUS: [&str; 3] = ["boundaries", "dependencies", "consequences"];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Reader {
    Product,
    Architecture,
}

fn items(value: &Value) -> &[Value] {
    value.as_array().map_or(&[], Vec::as_slice)
}

fn by_id<'a>(values: &'a Value, id: &Value) -> Option<&'a Value> {
    items(values).iter().find(|entry| &entry["id"] == id)
}

/// The fields of `value`, followed by `extra` in order; an object spread in the JS views.
fn extended(value: &Value, extra: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    let mut map = value.as_object().cloned().unwrap_or_default();
    for (key, item) in extra {
        map.insert(key.into(), item);
    }
    Value::Object(map)
}

fn text(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|s| !js_blank(s) && line_text(s, TEXT))
}

/// Up to 32 unique texts, nonempty unless `empty` is allowed.
fn texts(value: &Value, empty: bool) -> bool {
    let Some(values) = value.as_array() else {
        return false;
    };
    let unique: HashSet<&str> = values.iter().filter_map(Value::as_str).collect();
    values.len() <= PROBES
        && (empty || !values.is_empty())
        && values.iter().all(text)
        && unique.len() == values.len()
}

/// The QA charter view: declared probes linked to the ledger records that name them.
pub fn qa(
    contract: Option<&Value>,
    evidence: Option<&Value>,
    current: &Value,
    probes: Option<&Value>,
    blockers: bool,
) -> Result<Value> {
    let invalid = |at: &str, detail: &str| format!("qa:invalid-probes at {at}: {detail}");
    let Some(contract) = contract.filter(|c| truthy(*c)) else {
        return Err("qa:missing-contract".into());
    };
    let coverage = coverage(Some(contract), evidence, current, blockers)?;
    let probes = probes
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("probes", "not a list"))?;
    let size = serde_json::to_string(probes).map_or(usize::MAX, |json| json.len());
    if probes.len() > PROBES || size > PROBE_BYTES {
        return Err(invalid(
            "probes",
            &format!("up to {PROBES} probes and {PROBE_BYTES} bytes"),
        ));
    }
    let criteria = &contract["criteria"];
    let mut seen = HashSet::new();
    let mut projected = Vec::with_capacity(probes.len());
    for (index, probe) in probes.iter().enumerate() {
        let entry = format!("probes[{index}]");
        let Some(map) = probe.as_object() else {
            return Err(invalid(&entry, "not an object"));
        };
        if let Some(unknown) = map.keys().find(|key| !PROBE_FIELDS.contains(&key.as_str())) {
            let unknown: String = unknown
                .chars()
                .filter(|c| !c.is_control())
                .take(64)
                .collect();
            return Err(invalid(
                &entry,
                &format!(
                    "no field `{unknown}`; fields are {}",
                    PROBE_FIELDS.join(", ")
                ),
            ));
        }
        let lists = [
            ("criterionIds", false),
            ("flows", false),
            ("preconditions", true),
            ("actions", false),
            ("evidenceIds", true),
        ];
        let failed = if !text(&probe["id"]) {
            Some(("id", "text up to 1024 characters"))
        } else if probe["id"].as_str().is_some_and(|id| seen.contains(id)) {
            Some(("id", "another probe has this id"))
        } else if !text(&probe["risk"]) {
            Some(("risk", "text up to 1024 characters"))
        } else if !text(&probe["expectedBasis"]) {
            Some(("expectedBasis", "text up to 1024 characters"))
        } else if let Some((name, empty)) = lists
            .iter()
            .find(|(name, empty)| !texts(&probe[*name], *empty))
        {
            Some((
                *name,
                if *empty {
                    "a list of up to 32 unique texts"
                } else {
                    "a nonempty list of up to 32 unique texts"
                },
            ))
        } else if items(&probe["criterionIds"])
            .iter()
            .any(|id| by_id(criteria, id).is_none())
        {
            Some((
                "criterionIds",
                "names a criterion the contract does not declare",
            ))
        } else {
            None
        };
        if let Some((name, detail)) = failed {
            return Err(invalid(&format!("{entry}.{name}"), detail));
        }
        seen.insert(probe["id"].as_str().unwrap_or_default());
        let mut records = Vec::new();
        for (position, id) in items(&probe["evidenceIds"]).iter().enumerate() {
            let at = format!("{entry}.evidenceIds[{position}]");
            let record = by_id(&coverage["records"], id)
                .ok_or_else(|| invalid(&at, &format!("no ledger record {id}")))?;
            if record["checkId"] != probe["id"] {
                return Err(invalid(
                    &at,
                    &format!(
                        "record {id} has checkId {}; a probe's evidence records name the probe's id as their checkId",
                        record["checkId"]
                    ),
                ));
            }
            if !items(&record["criterionIds"])
                .iter()
                .all(|criterion| items(&probe["criterionIds"]).contains(criterion))
            {
                return Err(invalid(
                    &at,
                    &format!("record {id} covers a criterion the probe does not name"),
                ));
            }
            records.push(record.clone());
        }
        let execution = if records.is_empty() {
            "not-run"
        } else {
            "recorded"
        };
        projected.push(extended(
            probe,
            [("records", json!(records)), ("execution", json!(execution))],
        ));
    }
    let names = |probe: &Value, id: &Value| items(&probe["criterionIds"]).contains(id);
    let criteria: Vec<Value> = items(criteria)
        .iter()
        .map(|criterion| {
            let id = &criterion["id"];
            let records: Vec<&Value> = items(&coverage["records"])
                .iter()
                .filter(|record| items(&record["criterionIds"]).contains(id))
                .collect();
            let current = |execution: &str| {
                records
                    .iter()
                    .any(|r| r["freshness"] == "current" && r["execution"] == execution)
            };
            let status =
                by_id(&coverage["criteria"], id).map_or(Value::Null, |c| c["status"].clone());
            extended(
                criterion,
                [
                    ("acceptance", status),
                    (
                        "probeIds",
                        projected
                            .iter()
                            .filter(|probe| names(probe, id))
                            .map(|probe| probe["id"].clone())
                            .collect(),
                    ),
                    (
                        "evidenceIds",
                        records.iter().map(|r| r["id"].clone()).collect(),
                    ),
                    ("conflict", json!(current("failed") && current("passed"))),
                ],
            )
        })
        .collect();
    let unexplored: Vec<Value> = criteria
        .iter()
        .map(|criterion| &criterion["id"])
        .filter(|id| {
            !projected.iter().any(|probe| {
                names(probe, id)
                    && items(&probe["records"]).iter().any(|record| {
                        record["freshness"] == "current"
                            && items(&record["criterionIds"]).contains(id)
                            && matches!(record["execution"].as_str(), Some("passed" | "failed"))
                    })
            })
        })
        .cloned()
        .collect();
    let context = &contract["context"];
    Ok(json!({
        "assurance": coverage["assurance"],
        "contract": coverage["contract"],
        "binding": current,
        "intent": contract["intent"],
        "exclusions": contract["exclusions"],
        "questions": context["questions"],
        "sources": context["sources"],
        "constraints": context["constraints"],
        "decisions": contract["decisions"],
        "checks": context["checks"],
        "criteria": criteria,
        "probes": projected,
        "unexploredCriterionIds": unexplored,
        "coverage": coverage,
    }))
}

/// Constraints `focus` selects by field, each with its cited sources.
fn selected(contract: &Value, focus: Option<&Value>) -> Result<Map<String, Value>> {
    let empty = Map::new();
    let focus = match focus {
        None => &empty,
        Some(value) => value
            .as_object()
            .filter(|map| map.keys().all(|key| FOCUS.contains(&key.as_str())))
            .ok_or("product:invalid-focus")?,
    };
    let constraints = &contract["context"]["constraints"];
    let sources = &contract["context"]["sources"];
    let mut selected = Map::new();
    for field in FOCUS {
        let ids = match focus.get(field) {
            None => &[][..],
            Some(ids) => ids
                .as_array()
                .map(Vec::as_slice)
                .ok_or("product:invalid-reference")?,
        };
        let unique: HashSet<&str> = ids.iter().filter_map(Value::as_str).collect();
        if ids.len() > PROBES
            || unique.len() != ids.len()
            || ids.iter().any(|id| by_id(constraints, id).is_none())
        {
            return Err("product:invalid-reference".into());
        }
        let chosen = ids
            .iter()
            .filter_map(|id| by_id(constraints, id))
            .map(|constraint| {
                let cited = items(&constraint["sourceIds"])
                    .iter()
                    .map(|id| by_id(sources, id).cloned().unwrap_or(Value::Null))
                    .collect();
                extended(constraint, [("sources", Value::Array(cited))])
            })
            .collect();
        selected.insert(field.into(), Value::Array(chosen));
    }
    Ok(selected)
}

/// The product or architecture reader's view of the same contract and coverage.
pub fn reader(
    reader: Reader,
    contract: Option<&Value>,
    evidence: Option<&Value>,
    current: &Value,
    focus: Option<&Value>,
    blockers: bool,
) -> Result<Value> {
    let Some(contract) = contract.filter(|c| truthy(*c)) else {
        return Err("product:missing-contract".into());
    };
    let coverage = coverage(Some(contract), evidence, current, blockers)?;
    let mut selected = selected(contract, focus)?;
    let context = &contract["context"];
    let criteria: Vec<Value> = items(&contract["criteria"])
        .iter()
        .map(|criterion| {
            let acceptance = by_id(&coverage["criteria"], &criterion["id"]).cloned();
            extended(
                criterion,
                [("acceptance", acceptance.unwrap_or(Value::Null))],
            )
        })
        .collect();
    let gaps: Vec<Value> = items(&coverage["criteria"])
        .iter()
        .filter(|criterion| criterion["status"] != "verified")
        .cloned()
        .collect();
    let mut view = json!({
        "assurance": coverage["assurance"],
        "contract": coverage["contract"],
        "binding": current,
        "intent": contract["intent"],
        "exclusions": contract["exclusions"],
        "sources": context["sources"],
        "constraints": context["constraints"],
        "questions": context["questions"],
        "decisions": contract["decisions"],
        "discoveryCoverage": context["coverage"],
        "criteria": criteria,
        "coverage": coverage,
    });
    let map = view.as_object_mut().expect("view is an object");
    match reader {
        Reader::Product => {
            let unresolved: Vec<Value> = items(&contract["decisions"])
                .iter()
                .filter(|decision| decision["resolved"] != true)
                .cloned()
                .collect();
            map.insert(
                "dependencies".into(),
                selected.remove("dependencies").unwrap_or_default(),
            );
            map.insert("unresolvedDecisions".into(), json!(unresolved));
            map.insert("deliveryGaps".into(), json!(gaps));
        }
        Reader::Architecture => {
            map.append(&mut selected);
            let adr: Vec<Value> = items(&context["sources"])
                .iter()
                .filter(|source| source["kind"] == "adr")
                .cloned()
                .collect();
            map.insert("adrSources".into(), json!(adr));
            map.insert("evidenceGaps".into(), json!(gaps));
        }
    }
    Ok(view)
}
