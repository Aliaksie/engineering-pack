//! Discovery batches, bounded evidence packets and session lookup receipts.
use super::{request as search_request, scope};
use crate::{
    contracts::{js_length, safe_integer},
    hash,
};
use serde_json::{Value, json};

pub fn digest(value: &Value) -> String {
    hash(&serde_json::to_vec(value).expect("JSON values serialize"))
}
fn size(value: &Value) -> usize {
    serde_json::to_vec(value)
        .expect("JSON values serialize")
        .len()
}
fn values(value: &Value) -> &[Value] {
    value.as_array().map_or(&[], Vec::as_slice)
}
fn term(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|s| !s.is_empty() && js_length(s) <= 256 && !s.contains(['\r', '\n', '\0']))
}
fn metadata_error() -> String {
    "output metadata exceeds maxOutputBytes; split into smaller evidence requests".into()
}

pub fn request(input: &Value) -> Result<Value, String> {
    let mut normalized = input.clone();
    crate::contracts::normalize_numbers(&mut normalized);
    let input = &normalized;
    let map = input
        .as_object()
        .filter(|m| {
            m.keys().all(|k| {
                [
                    "terms",
                    "paths",
                    "maxFiles",
                    "maxMatches",
                    "maxBytes",
                    "maxOutputBytes",
                    "requests",
                ]
                .contains(&k.as_str())
            })
        })
        .ok_or("invalid bounded locator request")?;
    let max = map
        .get("maxOutputBytes")
        .map_or(Some(32768), safe_integer)
        .filter(|n| (256..=65536).contains(n))
        .ok_or("maxOutputBytes must be 256..65536")?;
    let mut deferred = Vec::new();
    let mut terms = Vec::new();
    if let Some(requests) = map.get("requests") {
        let requests = requests
            .as_array()
            .filter(|r| !r.is_empty() && r.len() <= 32 && !map.contains_key("terms"))
            .ok_or("use terms or 1..32 locator requests")?;
        let mut ids = Vec::new();
        for r in requests {
            let valid = r.as_object().is_some_and(|m| {
                m.keys()
                    .all(|k| ["id", "term", "dependsOn"].contains(&k.as_str()))
            }) && r["id"].as_str().is_some_and(|s| {
                !s.is_empty()
                    && s.len() <= 64
                    && s.bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            }) && !ids.contains(&r["id"])
                && term(&r["term"])
                && r.get("dependsOn")
                    .is_none_or(|v| v.as_array().is_some_and(|a| a.iter().all(Value::is_string)));
            if !valid {
                return Err("invalid locator-only request; side effects are unsupported".into());
            }
            ids.push(r["id"].clone());
            if !values(&r["dependsOn"]).is_empty() {
                deferred.push(json!({"id":r["id"],"reason":"dependent lookup requires a later reasoning stage"}));
            } else {
                terms.push(r["term"].clone());
            }
        }
        for r in requests {
            for dep in values(&r["dependsOn"]) {
                if !ids.contains(dep) || dep == &r["id"] {
                    return Err("invalid lookup dependency".into());
                }
            }
        }
    } else {
        terms = input["terms"]
            .as_array()
            .filter(|a| !a.is_empty() && a.len() <= 32 && a.iter().all(term))
            .ok_or("invalid locator terms")?
            .clone();
    }
    let mut unique = Vec::new();
    for term in terms {
        if !unique.contains(&term) {
            unique.push(term);
        }
    }
    let mut paths = Vec::new();
    if let Some(paths_input) = input.get("paths") {
        for p in paths_input.as_array().ok_or("paths must be an array")? {
            let p = json!(scope(p)?);
            if !paths.contains(&p) {
                paths.push(p);
            }
        }
    }
    let mut options = input.clone();
    for k in ["requests", "maxOutputBytes"] {
        options.as_object_mut().unwrap().remove(k);
    }
    options["paths"] = json!(paths);
    for (k, ceiling) in [
        ("maxFiles", 10000),
        ("maxMatches", 1000),
        ("maxBytes", 16777216),
    ] {
        if let Some(v) = options.get(k)
            && !safe_integer(v).is_some_and(|n| (1..=ceiling).contains(&n))
        {
            return Err(format!("invalid {k}"));
        }
    }
    // Validate the scanner's remaining contract even for a wholly deferred batch.
    let mut scan = options.clone();
    scan["terms"] = if unique.is_empty() {
        json!(["deferred"])
    } else {
        json!(unique)
    };
    search_request(&scan)?;
    Ok(
        json!({"options":options,"terms":unique,"paths":paths,"deferred":deferred,"maxOutputBytes":max}),
    )
}

pub fn deferred(request: &Value) -> Result<Value, String> {
    let max = request["maxOutputBytes"]
        .as_u64()
        .ok_or("invalid discovery request")?;
    let result = json!({"terms":[],"paths":request["paths"],"matches":[],"deferred":request["deferred"],"discovery":{"requested":values(&request["deferred"]).len(),"reused":0,"fresh":0,"batches":0},"truncated":false,"bounds":{"maxOutputBytes":max}});
    if size(&result) > max as usize {
        return Err(metadata_error());
    }
    Ok(result)
}

pub fn reuse(
    terms: &Value,
    paths: &Value,
    identity: &Value,
    state: &Value,
) -> Result<Value, String> {
    if !terms.is_array() || !paths.is_array() {
        return Err("invalid lookup request".into());
    }
    let (mut reused, mut fresh) = (Vec::new(), Vec::new());
    for term in values(terms) {
        let key = digest(&json!([term, paths]));
        let packet = &state["lookups"][key];
        if !identity.is_null()
            && packet["identity"] == *identity
            && packet["matches"].as_array().is_some_and(|matches| {
                matches.iter().all(|m| {
                    m.is_object()
                        && m["path"].is_string()
                        && safe_integer(&m["line"]).is_some_and(|n| n > 0)
                        && m["text"].is_string()
                        && m.get("snippetTruncated").is_none_or(Value::is_boolean)
                })
            })
        {
            reused.push(term.clone());
        } else {
            fresh.push(term.clone());
        }
    }
    Ok(json!({"reused":reused,"fresh":fresh}))
}

pub fn result(input: &Value) -> Result<Value, String> {
    let request = &input["request"];
    let scanned = &input["scanned"];
    let terms = values(&request["terms"]);
    let fresh = values(&input["fresh"]);
    let max = request["maxOutputBytes"]
        .as_u64()
        .ok_or("invalid discovery request")? as usize;
    let mut matches: Vec<Value> = Vec::new();
    let mut truncation = values(&scanned["truncation"]).to_vec();
    let mut bytes = 0;
    let mut append = |m: Value| {
        if let Some(previous) = matches
            .iter_mut()
            .find(|p| p["path"] == m["path"] && p["line"] == m["line"] && p["text"] == m["text"])
        {
            let mut indexes = values(&previous["termIndexes"]).to_vec();
            for i in values(&m["termIndexes"]) {
                if !indexes.contains(i) {
                    indexes.push(i.clone());
                }
            }
            indexes.sort_by_key(|i| i.as_u64());
            previous["termIndexes"] = json!(indexes);
            return;
        }
        let reason =
            if matches.len() >= request["options"]["maxMatches"].as_u64().unwrap_or(80) as usize {
                Some("maxMatches")
            } else if bytes + size(&m) > max - 256 {
                Some("maxOutputBytes")
            } else {
                None
            };
        if let Some(reason) = reason {
            let reason = json!(reason);
            if !truncation.contains(&reason) {
                truncation.push(reason);
            }
        } else {
            bytes += size(&m);
            matches.push(m);
        }
    };
    for term in values(&input["reused"]) {
        let key = digest(&json!([term, request["paths"]]));
        for m in values(&input["state"]["lookups"][key]["matches"]) {
            let mut m = m.clone();
            m["termIndexes"] = json!([terms.iter().position(|t| t == term).unwrap()]);
            append(m);
        }
    }
    for m in values(&scanned["matches"]) {
        let mut m = m.clone();
        let indexes: Result<Vec<_>, _> = values(&m["termIndexes"])
            .iter()
            .map(|index| {
                index
                    .as_u64()
                    .and_then(|i| fresh.get(i as usize))
                    .and_then(|t| terms.iter().position(|v| v == t))
                    .ok_or("invalid locator match index")
            })
            .collect();
        m["termIndexes"] = json!(indexes?);
        append(m);
    }
    let mut result = scanned.clone();
    if !result.is_object() {
        return Err("invalid discovery scan".into());
    }
    result["terms"] = json!(terms);
    result["matches"] = json!(matches);
    result["deferred"] = request["deferred"].clone();
    result["truncated"] = json!(!truncation.is_empty());
    result["truncation"] = json!(truncation);
    result["discovery"] = json!({"requested":terms.len()+values(&request["deferred"]).len(),"reused":values(&input["reused"]).len(),"fresh":fresh.len(),"batches":usize::from(!fresh.is_empty())});
    result["reuse"] = json!({"enabled":input["enabled"],"identity":input["identity"],"validated":input["stable"]==true,"diagnostic":input["diagnostic"],"invalidation":"session/task, HEAD, scoped file inventory/versions and root manifests/config; unavailable evidence always scans fresh"});
    if !result["bounds"].is_object() {
        result["bounds"] = json!({});
    }
    result["bounds"]["maxOutputBytes"] = json!(max);
    while size(&result) > max && !values(&result["matches"]).is_empty() {
        result["matches"].as_array_mut().unwrap().pop();
        result["truncated"] = json!(true);
        if !values(&result["truncation"]).contains(&json!("maxOutputBytes")) {
            result["truncation"]
                .as_array_mut()
                .unwrap()
                .push(json!("maxOutputBytes"));
        }
    }
    if size(&result) > max {
        return Err(metadata_error());
    }
    Ok(result)
}

pub fn update(mut current: Value, input: &Value) -> Result<Value, String> {
    if !current["activity"].is_object() || !current["lookups"].is_object() {
        return Err("invalid discovery state".into());
    }
    let request = &input["request"];
    let terms = values(&request["terms"]);
    let paths = &request["paths"];
    let identity = &input["identity"];
    let fresh = values(&input["fresh"]);
    let scanned = &input["scanned"];
    let key = digest(&json!([identity, terms, paths]));
    let repeats =
        if !identity.is_null() && fresh.is_empty() && current["lastDiscovery"]["key"] == key {
            current["lastDiscovery"]["repetitions"]
                .as_u64()
                .unwrap_or(0)
                + 1
        } else {
            0
        };
    current["lastDiscovery"] = json!({"key":key,"repetitions":repeats});
    let repeated = terms
        .iter()
        .filter(|t| !input["state"]["lookups"][digest(&json!([t, paths]))].is_null())
        .count();
    for (k, n) in [
        ("discoveryRequests", terms.len()),
        ("repeatedLookups", repeated),
        ("reuseHits", values(&input["reused"]).len()),
        ("freshLookups", fresh.len()),
        ("batchedOperations", usize::from(!fresh.is_empty())),
    ] {
        let previous = current["activity"][k].as_u64().unwrap_or(0);
        let next = previous + n as u64;
        if next > 9_007_199_254_740_991 {
            return Err("session counter overflow".into());
        }
        current["activity"][k] = json!(next);
    }
    if input["stable"] == true
        && scanned["truncated"] != true
        && scanned["skipped"]
            .as_object()
            .is_none_or(|m| m.values().all(|v| v.as_u64().unwrap_or(0) == 0))
    {
        for (index, term) in fresh.iter().enumerate() {
            let matches: Vec<_> = values(&scanned["matches"])
                .iter()
                .filter(|m| values(&m["termIndexes"]).contains(&json!(index)))
                .map(|m| {
                    let mut m = m.clone();
                    m.as_object_mut().unwrap().remove("termIndexes");
                    m
                })
                .collect();
            current["lookups"][digest(&json!([term, paths]))] =
                json!({"identity":identity,"matches":matches});
        }
        let lookups = current["lookups"]
            .as_object_mut()
            .ok_or("invalid lookup state")?;
        while lookups.len() > 32 {
            let key = lookups.keys().next().unwrap().clone();
            lookups.shift_remove(&key);
        }
        while size(&json!(lookups)) > 65536 {
            let key = lookups.keys().next().unwrap().clone();
            lookups.shift_remove(&key);
        }
    }
    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frozen_discovery_contracts() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/runtime/discovery-contracts.json"
        ))
        .unwrap();
        for case in corpus["cases"].as_array().unwrap() {
            let i = &case["request"]["input"];
            let actual = match case["request"]["operation"].as_str().unwrap() {
                "discovery-request" => request(&i["input"]),
                "discovery-deferred" => deferred(&i["request"]),
                "lookup-reuse" => reuse(&i["terms"], &i["paths"], &i["identity"], &i["state"]),
                "discovery-result" => result(i),
                "discovery-state" => {
                    if i["current"].is_object() {
                        update(i["current"].clone(), &i["input"])
                    } else {
                        Err("invalid discovery state".into())
                    }
                }
                other => panic!("{other}"),
            };
            if case["response"]["ok"] == true {
                assert_eq!(
                    actual.unwrap(),
                    case["response"]["result"],
                    "{}",
                    case["id"]
                );
            } else {
                assert!(actual.is_err(), "{}", case["id"]);
            }
        }
    }
}
