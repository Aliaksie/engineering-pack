//! Read-only context composition from explicitly observed local citations.
use crate::{contracts, digest, routing, session::acceptance};
use serde_json::{Value, json};
fn items(v: &Value) -> &[Value] {
    v.as_array().map_or(&[], Vec::as_slice)
}
fn member(file: &str, scope: &str) -> bool {
    scope == "." || file == scope || file.starts_with(&format!("{scope}/"))
}
fn overlaps(entry: &Value, scope: &[String]) -> bool {
    items(&entry["scope"]).iter().any(|area| {
        area.as_str().is_some_and(|area| {
            scope
                .iter()
                .any(|file| member(file, area) || member(area, file))
        })
    })
}
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(v) => *v,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}
fn local(source: &Value) -> bool {
    source["kind"] != "external" && source["kind"] != "request"
}
pub fn request(input: &Value) -> Result<Value, String> {
    let scope = input["scope"]
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= 32)
        .ok_or("grounding:capacity")?;
    let sources = input.get("sources").unwrap_or(&Value::Null);
    if input.get("sources").is_some() && !sources.is_array() || items(sources).len() > 24 {
        return Err("grounding:capacity".into());
    }
    let mut normalized = Vec::<String>::new();
    let mut files = vec![".craft/project.json".to_string()];
    for value in scope {
        let path = contracts::relative_file(value)?;
        if !normalized.contains(&path) {
            normalized.push(path);
        }
    }
    let mut add = |file: String| {
        if !files.contains(&file) {
            files.push(file);
        }
    };
    for file in &normalized {
        if routing::private_path(file) {
            return Err("grounding:private-source".into());
        }
        add("AGENTS.md".into());
        if file != "." {
            let parts: Vec<_> = file.split('/').collect();
            for i in 1..=parts.len() {
                add(format!("{}/AGENTS.md", parts[..i].join("/")));
            }
        }
    }
    for source in items(sources) {
        if local(source) && overlaps(source, &normalized) {
            let file = contracts::relative_file(&source["reference"])?;
            if routing::private_path(&file) {
                return Err("grounding:private-source".into());
            }
            add(file);
        }
    }
    if files.len() > 64 {
        return Err("grounding:capacity".into());
    }
    Ok(json!({"scope":normalized,"files":files}))
}
fn question(packet: &mut Value, text: String) {
    let q = packet["questions"].as_array_mut().unwrap();
    if !q.contains(&json!(text)) {
        q.push(json!(text));
    }
}
pub fn compose(input: &Value) -> Result<Value, String> {
    contracts::validate_project(&input["project"])?;
    let normalized = request(input)?;
    let scope: Vec<String> = items(&normalized["scope"])
        .iter()
        .map(|v| v.as_str().unwrap().into())
        .collect();
    let array = |key: &str| input.get(key).cloned().unwrap_or_else(|| json!([]));
    let mut packet = json!({"formatVersion":1,"scope":scope,"sources":array("sources"),"constraints":array("constraints"),"checks":array("checks"),"questions":array("questions"),"coverage":"unknown"});
    acceptance::validate_context(&packet)?;
    for key in ["sources", "constraints", "checks"] {
        packet[key]
            .as_array_mut()
            .unwrap()
            .retain(|entry| overlaps(entry, &scope));
    }
    let known: Vec<Value> = items(&packet["sources"])
        .iter()
        .map(|s| s["id"].clone())
        .collect();
    if ["constraints", "checks"].iter().any(|key| {
        items(&packet[key]).iter().any(|entry| {
            items(&entry["sourceIds"])
                .iter()
                .any(|id| !known.contains(id))
        })
    }) {
        return Err("grounding:basis-outside-scope".into());
    }
    if items(&packet["constraints"]).iter().any(|entry| {
        items(&entry["conflicts"]).iter().any(|id| {
            !items(&packet["constraints"])
                .iter()
                .any(|other| other["id"] == *id)
        })
    }) {
        return Err("grounding:conflict-outside-scope".into());
    }
    let observations = items(&input["observations"]);
    for observation in observations {
        let reference = observation["reference"]
            .as_str()
            .ok_or("invalid grounding observation")?;
        if observation["status"] == "unavailable" {
            question(&mut packet, format!("Source unavailable: {reference}"));
        }
        if observation["status"] != "observed" {
            continue;
        }
        let mut existing = false;
        let mut changed = false;
        for source in packet["sources"].as_array_mut().unwrap() {
            if source["reference"] == reference && local(source) {
                existing = true;
                if source["identity"] != observation["identity"] {
                    source["identity"] = Value::Null;
                    source["status"] = json!("unknown");
                    changed = true;
                }
            }
        }
        if changed {
            question(&mut packet, format!("Source changed: {reference}"));
        }
        if !existing && (reference.ends_with("AGENTS.md") || reference == ".craft/project.json") {
            let area = if reference != ".craft/project.json" {
                reference.rsplit_once('/').map_or(".", |(parent, _)| parent)
            } else {
                "."
            };
            packet["sources"].as_array_mut().unwrap().push(json!({"id":format!("local-{}",&digest(&json!(reference))[..16]),"kind":if reference==".craft/project.json"{"configuration"}else{"instruction"},"reference":reference,"identity":observation["identity"],"observedAt":input["observedAt"],"scope":[area],"provenance":"observed","status":"unknown"}));
        }
    }
    let mut unavailable = Vec::new();
    for source in packet["sources"].as_array_mut().unwrap() {
        if local(source)
            && !observations.iter().any(|entry| {
                entry["reference"] == source["reference"] && entry["status"] == "observed"
            })
        {
            source["identity"] = Value::Null;
            source["status"] = json!("unknown");
            unavailable.push(format!(
                "Source unavailable: {}",
                source["reference"].as_str().unwrap()
            ));
        }
    }
    for text in unavailable {
        question(&mut packet, text);
    }
    let sources = packet["sources"].clone();
    for constraint in packet["constraints"].as_array_mut().unwrap() {
        if constraint["status"] == "accepted"
            && items(&constraint["sourceIds"]).iter().any(|id| {
                items(&sources).iter().any(|s| {
                    s["id"] == *id && (s["identity"].is_null() || s["status"] != "accepted")
                })
            })
        {
            constraint["status"] = json!("unknown");
        }
    }
    if let Some(configuration) = items(&sources)
        .iter()
        .find(|entry| entry["reference"] == ".craft/project.json")
    {
        for module in routing::matching_modules(&input["project"], &scope) {
            let description = json!({"module":module["name"],"owner":module["owner"],"dependsOn":module.get("dependsOn").cloned().unwrap_or_else(||json!([]))});
            packet["constraints"].as_array_mut().unwrap().push(json!({"id":format!("module-{}",module["name"].as_str().unwrap()),"description":description.to_string(),"sourceIds":[configuration["id"]],"authority":"project","status":if truthy(&configuration["identity"]){"accepted"}else{"unknown"},"scope":[module["path"]],"required":false,"conflicts":[]}));
            for check in items(&module["checks"]) {
                packet["checks"].as_array_mut().unwrap().push(json!({"id":format!("declared-{}",&digest(&json!([module["name"],check]))[..16]),"description":json!({"cwd":module["path"],"command":check["command"],"args":check.get("args").cloned().unwrap_or_else(||json!([]))}).to_string(),"sourceIds":[configuration["id"]],"scope":[module["path"]]}));
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    packet["checks"]
        .as_array_mut()
        .unwrap()
        .retain(|entry| seen.insert(entry.to_string()));
    if !truthy(&input["binding"]["identity"]) {
        question(
            &mut packet,
            "Repository identity unavailable; source freshness is unknown.".into(),
        );
    }
    let discovery = &input["discovery"];
    if truthy(discovery)
        && (truthy(&discovery["truncated"])
            || discovery["skipped"]
                .as_object()
                .is_some_and(|v| v.values().any(truthy)))
    {
        question(
            &mut packet,
            "Discovery coverage is incomplete; matches do not prove absence.".into(),
        );
    }
    if items(&packet["sources"]).is_empty() {
        question(
            &mut packet,
            "No grounded sources supplied or observed.".into(),
        );
    }
    packet["coverage"] = json!(if items(&packet["questions"]).is_empty() {
        "unknown"
    } else {
        "incomplete"
    });
    acceptance::validate_context(&packet)?;
    Ok(packet)
}
