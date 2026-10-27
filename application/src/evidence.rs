use craft_engine::hash;
use craft_platform::{
    providers::{ACTIVE, extract, for_path},
    repository::{Inventory, database_guard, eligible, now, read_source},
    state,
    store::Store,
};
use serde_json::{Value, json};
use std::{path::Path, time::Instant};

pub fn run(operation: &str, input: &Value) -> Result<Value, String> {
    let mut budget = if operation == "refresh" {
        Some(super::budget::Budget::from_input(input)?)
    } else {
        None
    };
    let root = state::canonical_root(Path::new(input["root"].as_str().ok_or("root is required")?))?;
    let expected = state::location(&root, None)?
        .directory
        .join("project-evidence.sqlite");
    let database = input["database"]
        .as_str()
        .map(Path::new)
        .unwrap_or(&expected);
    if database != expected {
        return Err("database must belong to the selected worktree shared state".into());
    }
    let _guard = database_guard(database)?;
    let mut store = Store::open(database)?;
    match operation {
        "status" => store.status(),
        "backup" => {
            let backup = database.with_extension("sqlite.backup");
            let _backup_guard = database_guard(&backup)?;
            store.backup(&backup)?;
            Ok(json!({"backup":backup,"coverage":store.status()?}))
        }
        "rebuild" => {
            store.clear()?;
            Ok(json!({"cleared":true,"next":"refresh","coverage":store.status()?}))
        }
        "query" => {
            let source_path = input
                .get("sourcePath")
                .map(|value| {
                    value
                        .as_str()
                        .ok_or("sourcePath must be a relative source path")
                })
                .transpose()?;
            let mut result = store.query(
                input["field"].as_str().unwrap_or("name"),
                input["value"].as_str().ok_or("query value required")?,
                input["after"].as_str().unwrap_or(""),
                input["limit"].as_u64().unwrap_or(30) as usize,
                source_path,
            )?;
            if input["verify"] == true {
                let checked_at = now();
                let mut checked = std::collections::HashMap::new();
                for row in result["results"]
                    .as_array_mut()
                    .ok_or("invalid query results")?
                {
                    let path = row["path"].as_str().ok_or("invalid source path")?;
                    let current = checked
                        .entry(path.to_string())
                        .or_insert_with(|| read_source(&root, path).ok().map(|b| hash(&b)));
                    row["verification"] = json!(if current.as_deref() == row["hash"].as_str() {
                        "verified-selected-source"
                    } else {
                        "stale-or-unavailable"
                    });
                    row["verifiedAt"] = json!(checked_at);
                }
            }
            result["scope"] = json!(root);
            Ok(result)
        }
        "refresh" => {
            let budget = budget.as_mut().unwrap();
            let mut reasons = std::collections::BTreeMap::<&str, u64>::new();
            let started = Instant::now();
            let retired = store.retain_providers(ACTIVE)?;
            let epoch = store.begin_inventory()?;
            let mut inventory = Inventory::start(&root)?;
            let (mut files, mut reused, mut changed, mut failed, mut bytes, mut deferred) =
                (0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
            while let Some(path) = inventory.next_path()? {
                if !eligible(&path) {
                    continue;
                }
                // Git's tracked inventory includes deleted files; completion retires them.
                if !root.join(&path).try_exists().map_err(|e| e.to_string())? {
                    continue;
                }
                let provider = for_path(&path).ok_or("eligible source has no provider")?;
                files += 1;
                let observed_at = now();
                let content = match read_source(&root, &path) {
                    Ok(content) => content,
                    Err(error) => {
                        store.fail(&path, provider, "", &error, &observed_at, epoch)?;
                        failed += 1;
                        continue;
                    }
                };
                bytes += content.len() as u64;
                let digest = hash(&content);
                if store.reuse(&path, provider, &digest, epoch)? {
                    reused += 1;
                    continue;
                }
                if let Err(reason) = budget.start(content.len() as u64, started.elapsed()) {
                    store.defer(&path, provider, epoch)?;
                    *reasons.entry(reason).or_default() += 1;
                    deferred += 1;
                    continue;
                }
                match extract(&path, &content) {
                    Ok(extraction) => {
                        if !budget.accept(&extraction)? {
                            store.defer(&path, provider, epoch)?;
                            *reasons.entry("maxOutputBytes").or_default() += 1;
                            deferred += 1;
                            continue;
                        }
                        store.replace(&extraction, &observed_at, epoch)?;
                        changed += 1;
                    }
                    Err(error) => {
                        store.fail(&path, provider, &digest, &error, &observed_at, epoch)?;
                        failed += 1;
                    }
                }
            }
            let removed = store.finish_inventory(epoch)?;
            Ok(
                json!({"coverage":store.status()?,"files":files,"reused":reused,"changed":changed,"failed":failed,"removed":removed,
                "deferred":deferred,"deferredByLimit":reasons,"extractionAttempts":budget.attempted,
                "extractionBytes":budget.bytes,"outputBytes":budget.output_bytes,
                "retiredSources":retired,"bytesScanned":bytes,"wallTimeMs":started.elapsed().as_millis(),"scope":root}),
            )
        }
        _ => Err("unknown evidence operation".into()),
    }
}

const OPERATIONS: &[&str] = &["version", "refresh", "status", "query", "backup", "rebuild"];
const USAGE: &str = "project-evidence version|refresh|status|query|backup|rebuild --enable --root <repository> [--member <id>=<absolute-root> ...] [--field id|name|path|target --value <exact-value> --limit 30 --after <id> --source <relative-path> --verify] [--max-files N --max-bytes N --max-output-bytes N --max-duration-ms N] [--json]";

/// `craft project-evidence <operation> ...`: explicit opt-in Project Intelligence. Without
/// `--enable` nothing is read or written. Exits 0, or 1 with `CODE: message` on stderr.
pub fn main(args: &[String]) -> i32 {
    let (code, stdout, stderr) = command(args);
    print!("{stdout}");
    eprint!("{stderr}");
    code
}

fn command(args: &[String]) -> (i32, String, String) {
    if args.is_empty() || args.iter().any(|arg| arg == "--help") {
        return (0, format!("{USAGE}\n"), String::new());
    }
    let (json, result) = match request(args) {
        Ok((json, None)) => (
            json,
            Ok(json!({"enabled": false, "reason": "Project evidence is disabled"})),
        ),
        Ok((json, Some((operation, input)))) => {
            let response = crate::evaluate(
                json!({"formatVersion": craft_engine::PROTOCOL_VERSION, "operation": operation, "input": input}),
            );
            let result = if response["ok"] == true {
                Ok(response["result"].clone())
            } else {
                let error = &response["error"];
                Err(format!(
                    "{}: {}",
                    error["code"].as_str().unwrap_or("NATIVE_FAILED"),
                    error["message"]
                        .as_str()
                        .unwrap_or("Native evidence failed")
                ))
            };
            (json, result)
        }
        Err(message) => (false, Err(format!("PROJECT_EVIDENCE_ERROR: {message}"))),
    };
    match result {
        Ok(result) => (0, render(&result, json), String::new()),
        Err(message) => (1, String::new(), format!("{message}\n")),
    }
}

/// Whether `--json` was given, and the native operation with its input when enabled.
type Request = (bool, Option<(String, Value)>);

fn request(args: &[String]) -> Result<Request, String> {
    let (operation, mut rest) = (args[0].clone(), args[1..].iter());
    let (mut options, mut members, mut flags) = (serde_json::Map::new(), Vec::new(), Vec::new());
    const VALUES: &[&str] = &[
        "--root",
        "--field",
        "--value",
        "--limit",
        "--after",
        "--source",
        "--max-files",
        "--max-bytes",
        "--max-output-bytes",
        "--max-duration-ms",
    ];
    while let Some(key) = rest.next() {
        let key = key.as_str();
        if matches!(key, "--enable" | "--verify" | "--json") {
            flags.push(key);
        } else if key == "--member"
            && let Some(value) = rest.next()
        {
            match value.split_once('=') {
                Some((id, root)) if !id.is_empty() => members.push(json!({"id": id, "root": root})),
                _ => return Err("member must be id=absolute-root".into()),
            }
        } else if VALUES.contains(&key)
            && let Some(value) = rest.next()
        {
            options.insert(key[2..].to_string(), json!(value));
        } else {
            return Err(format!("unknown or incomplete option: {key}"));
        }
    }
    let number = |name: &str| {
        options
            .get(name)
            .and_then(Value::as_str)
            .map(|text| crate::discovery::number(text).as_f64())
    };
    let integral = |value: Option<f64>, max: f64| {
        value.filter(|n| n.fract() == 0.0 && (1.0..=max).contains(n))
    };
    let limit = match number("limit") {
        None => 30.0,
        given => integral(given.flatten(), 200.0).ok_or("limit must be between 1 and 200")?,
    };
    let mut input = serde_json::Map::new();
    if !members.is_empty() {
        input.insert("members".into(), Value::Array(members));
    }
    for (flag, field) in [
        ("max-files", "maxFiles"),
        ("max-bytes", "maxBytes"),
        ("max-output-bytes", "maxOutputBytes"),
        ("max-duration-ms", "maxDurationMs"),
    ] {
        if let Some(value) = number(flag) {
            let value = integral(value, 9_007_199_254_740_991.0)
                .ok_or(format!("{flag} must be a positive safe integer"))?;
            input.insert(field.into(), json!(value as u64));
        }
    }
    for (option, field) in [
        ("source", "sourcePath"),
        ("field", "field"),
        ("value", "value"),
        ("after", "after"),
    ] {
        if let Some(value) = options.get(option) {
            input.insert(field.into(), value.clone());
        }
    }
    input.insert("limit".into(), json!(limit as u64));
    input.insert("verify".into(), json!(flags.contains(&"--verify")));
    let json = flags.contains(&"--json");
    if !flags.contains(&"--enable") {
        return Ok((json, None));
    }
    if !OPERATIONS.contains(&operation.as_str()) {
        return Err("unknown project evidence operation".into());
    }
    let root = options.get("root").and_then(Value::as_str).unwrap_or(".");
    input.insert(
        "root".into(),
        json!(crate::project::absolute(root).to_string_lossy()),
    );
    Ok((json, Some((operation, Value::Object(input)))))
}

fn render(result: &Value, json: bool) -> String {
    let Some(rows) = result["results"].as_array().filter(|_| !json) else {
        return if json {
            format!("{result}\n")
        } else {
            format!(
                "{}\n",
                serde_json::to_string_pretty(result).expect("JSON values serialize")
            )
        };
    };
    let coverage = &result["coverage"];
    let mut text = format!(
        "{} result(s); {}; inventory {}\n",
        rows.len(),
        crate::cli::shown(&coverage["freshness"]),
        if coverage["inventoryComplete"] == true {
            "complete"
        } else {
            "partial"
        }
    );
    for row in rows {
        let evidence = &row["evidence"];
        let verification = row["verification"].as_str().unwrap_or("stored-snapshot");
        text.push_str(&format!(
            "{}:{}:{} {} {} [{verification}]\n",
            crate::cli::shown(&row["path"]),
            crate::cli::shown(&evidence["line"]),
            crate::cli::shown(&evidence["column"]),
            crate::cli::shown(&evidence["kind"]),
            crate::cli::shown(&evidence["name"]),
        ));
    }
    if let Some(after) = result["continuation"].as_str() {
        text.push_str(&format!("Continue with --after {after}\n"));
    }
    text
}

#[cfg(test)]
mod command_tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn numbers_follow_javascript_number_and_bounds() {
        let input = |limit: &str| {
            request(&args(&["query", "--enable", "--limit", limit]))
                .map(|(_, request)| request.unwrap().1)
        };
        assert_eq!(input(" 0x10 ").unwrap()["limit"], 16);
        assert_eq!(input("1e2").unwrap()["limit"], 100);
        for invalid in ["", "0", "201", "2.5", "abc", "Infinity"] {
            assert_eq!(
                input(invalid).unwrap_err(),
                "limit must be between 1 and 200"
            );
        }
        let budget = |value: &str| request(&args(&["refresh", "--enable", "--max-files", value]));
        assert_eq!(
            budget("9007199254740991").unwrap().1.unwrap().1["maxFiles"],
            9_007_199_254_740_991u64
        );
        assert_eq!(
            budget("9007199254740992").unwrap_err(),
            "max-files must be a positive safe integer"
        );
    }

    #[test]
    fn disabled_access_names_no_operation_and_reads_nothing() {
        let (json, enabled) = request(&args(&["bogus", "--root", "/missing", "--json"])).unwrap();
        assert!(json && enabled.is_none());
        assert_eq!(
            request(&args(&["bogus", "--enable"])).unwrap_err(),
            "unknown project evidence operation"
        );
        let (code, stdout, stderr) = command(&args(&["status"]));
        assert_eq!((code, stderr.as_str()), (0, ""));
        assert_eq!(
            serde_json::from_str::<Value>(&stdout).unwrap()["enabled"],
            false
        );
    }

    #[test]
    fn options_are_validated_before_access() {
        for (list, message) in [
            (
                &["query", "--binary", "/x"][..],
                "unknown or incomplete option: --binary",
            ),
            (
                &["query", "--limit"][..],
                "unknown or incomplete option: --limit",
            ),
            (
                &["query", "--member"][..],
                "unknown or incomplete option: --member",
            ),
            (
                &["query", "--member", "=x"][..],
                "member must be id=absolute-root",
            ),
        ] {
            let (code, stdout, stderr) = command(&args(list));
            assert_eq!((code, stdout.as_str()), (1, ""));
            assert_eq!(stderr, format!("PROJECT_EVIDENCE_ERROR: {message}\n"));
        }
        let (_, request) = request(&args(&[
            "query", "--enable", "--member", "a=b=c", "--verify",
        ]))
        .unwrap();
        let input = request.unwrap().1;
        assert_eq!(input["members"], json!([{"id": "a", "root": "b=c"}]));
        assert_eq!(input["verify"], true);
        assert!(Path::new(input["root"].as_str().unwrap()).is_absolute());
    }

    #[test]
    fn query_results_render_one_line_each() {
        let result = json!({"results": [{"path": "a.js", "evidence": {"line": 2, "column": 8, "kind": "js:declares", "name": "A"}}],
            "coverage": {"freshness": "stored-snapshot", "inventoryComplete": false}, "continuation": "next"});
        assert_eq!(
            render(&result, false),
            "1 result(s); stored-snapshot; inventory partial\na.js:2:8 js:declares A [stored-snapshot]\nContinue with --after next\n"
        );
        assert_eq!(render(&result, true), format!("{result}\n"));
    }
}
