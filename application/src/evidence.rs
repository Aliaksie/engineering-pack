use crate::failure::Failure;
use crate::telemetry::Recorder;
use craft_engine::hash;
use craft_platform::{
    providers::{ACTIVE, extract, for_path},
    repository::{Inventory, database_guard, eligible, now, read_source},
    state,
    store::{Query, Store},
};
use serde_json::{Value, json};
use std::{path::Path, time::Instant};

pub fn run(operation: &str, input: &Value) -> Result<Value, Failure> {
    let mut budget = if operation == "refresh" {
        Some(super::budget::Budget::from_input(input)?)
    } else {
        None
    };
    let root = state::canonical_root(Path::new(input["root"].as_str().ok_or("root is required")?))?;
    let expected = state::shared(&root, "intelligence", true)?.join("project.sqlite");
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
        "status" => Ok(store.status()?),
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
            let text = |name: &str| {
                input
                    .get(name)
                    .map(|value| value.as_str().ok_or(format!("{name} must be a string")))
                    .transpose()
            };
            let source = input
                .get("sourcePath")
                .map(|value| {
                    value
                        .as_str()
                        .ok_or("sourcePath must be a relative source path")
                })
                .transpose()?;
            let mut result = store.query(&Query {
                field: input["field"].as_str().unwrap_or("name"),
                value: input["value"].as_str().ok_or("query value required")?,
                after: input["after"].as_str().unwrap_or(""),
                limit: input["limit"].as_u64().unwrap_or(30) as usize,
                source,
                kind: text("kind")?,
                under: text("under")?,
            })?;
            let excerpt = input["excerpt"] == true;
            if input["verify"] == true || excerpt {
                verify(&root, &mut result, excerpt)?;
            }
            result["scope"] = json!(root);
            Ok(result)
        }
        "refresh" => {
            let budget = budget.as_mut().unwrap();
            let mut reasons = std::collections::BTreeMap::<&str, u64>::new();
            let started = Instant::now();
            let exclusions = craft_engine::contracts::evidence_exclusions(
                &craft_platform::documents::project(&root)?,
            )?;
            let retired = store.retain_providers(ACTIVE)?;
            let epoch = store.begin_inventory()?;
            let mut inventory = Inventory::start(&root, &exclusions)?;
            store.begin_batch()?;
            let (mut files, mut reused, mut changed, mut failed, mut bytes, mut deferred) =
                (0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
            let mut excluded = 0u64;
            while let Some(path) = inventory.next_path()? {
                if !eligible(&path) {
                    continue;
                }
                if craft_engine::contracts::excluded(&path, &exclusions) {
                    excluded += 1;
                    continue;
                }
                // Git's tracked inventory includes deleted files; completion retires them.
                if !root.join(&path).try_exists()? {
                    continue;
                }
                let provider = for_path(&path).ok_or("eligible source has no provider")?;
                files += 1;
                let observed_at = now();
                let content = match read_source(&root, &path) {
                    Ok(content) => content,
                    Err(error) => {
                        store.fail(&path, provider, "", &error.to_string(), &observed_at, epoch)?;
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
            store.commit_batch()?;
            let removed =
                store.finish_inventory(epoch, &inventory.describe(), &exclusions, excluded)?;
            Ok(
                json!({"coverage":store.status()?,"files":files,"reused":reused,"changed":changed,"failed":failed,"removed":removed,"excluded":excluded,
                "deferred":deferred,"deferredByLimit":reasons,"extractionAttempts":budget.attempted,
                "extractionBytes":budget.bytes,"outputBytes":budget.output_bytes,
                "retiredSources":retired,"bytesScanned":bytes,"wallTimeMs":started.elapsed().as_millis(),"scope":root}),
            )
        }
        _ => Err("unknown evidence operation".into()),
    }
}

/// Rehash each cited source once and mark it verified or stale. A row from a verified source
/// spanning several lines gains its `endLine`; with `excerpt` it also carries its trimmed
/// first line. Stale sources give neither.
fn verify(root: &Path, result: &mut Value, excerpt: bool) -> Result<(), Failure> {
    let checked_at = now();
    let mut current = std::collections::HashMap::new();
    let sources = result["sources"]
        .as_object_mut()
        .ok_or("invalid query sources")?;
    for (path, source) in sources.iter_mut() {
        let content = read_source(root, path)
            .ok()
            .filter(|bytes| source["hash"].as_str() == Some(hash(bytes).as_str()));
        source["verification"] = json!(if content.is_some() {
            "verified-selected-source"
        } else {
            "stale-or-unavailable"
        });
        source["verifiedAt"] = json!(checked_at);
        if let Some(content) = content {
            current.insert(path.clone(), content);
        }
    }
    for row in result["results"]
        .as_array_mut()
        .ok_or("invalid query results")?
    {
        let (Some(content), Some(line), Some(start), Some(end)) = (
            row["path"].as_str().and_then(|path| current.get(path)),
            row["line"].as_u64(),
            row["startByte"].as_u64(),
            row["endByte"].as_u64(),
        ) else {
            continue;
        };
        let (start, end) = (start as usize, end as usize);
        if let Some(span) = content.get(start..end) {
            let lines = span.iter().filter(|&&b| b == b'\n').count() as u64;
            if lines > 0 {
                row["endLine"] = json!(line + lines);
            }
        }
        if excerpt {
            row["excerpt"] = json!(craft_engine::values::line_excerpt(
                content,
                start,
                EXCERPT_CHARS
            ));
        }
    }
    Ok(())
}

/// Longest excerpt, in characters; a longer line is cut and marked with `…`.
const EXCERPT_CHARS: usize = 160;

const OPERATIONS: &[&str] = &["version", "refresh", "status", "query", "backup", "rebuild"];
const USAGE: &str = "project-evidence version|refresh|status|query|backup|rebuild --enable --root <directory> [--member <id>=<absolute-root> ...] [--field id|name|path|target --value <exact-value> --kind <kind> --under <directory> --source <relative-path> --limit 30 --after <id> --verify --excerpt] [--max-files N --max-bytes N --max-output-bytes N --max-duration-ms N] [--json]";

/// `craft project-evidence <operation> ...`: explicit opt-in Project Intelligence. Without
/// `--enable` nothing is read or written. Exits 0, or 1 with `CODE: message` on stderr.
pub fn main(args: &[String]) -> i32 {
    let (code, stdout, stderr, export) = command(args);
    print!("{stdout}");
    eprint!("{stderr}");
    Recorder::deliver(export);
    code
}

fn command(args: &[String]) -> (i32, String, String, Option<Value>) {
    if args.is_empty() || args.iter().any(|arg| arg == "--help") {
        return (0, format!("{USAGE}\n"), String::new(), None);
    }
    let mut export = None;
    let (json, result) = match request(args) {
        Ok((json, None)) => (
            json,
            Ok(json!({"enabled": false, "reason": "Project evidence is disabled"})),
        ),
        Ok((json, Some((operation, input)))) => {
            let root = input["root"].as_str().unwrap_or(".");
            let mut recorder = Recorder::open(Path::new(root), None, None);
            let command = recorder.start("cli.command", None);
            let result = crate::project_evidence(&operation, &input);
            match &result {
                Ok(_) => recorder.complete(
                    command,
                    serde_json::Map::from_iter([("command".into(), json!("project-evidence"))]),
                    None,
                ),
                Err(failure) => recorder.complete(
                    command,
                    serde_json::Map::new(),
                    Some(failure.code("INVALID_INPUT")),
                ),
            }
            export = recorder.finish();
            (
                json,
                result.map_err(|failure| format!("INVALID_INPUT: {}", failure.message)),
            )
        }
        Err(message) => (false, Err(format!("PROJECT_EVIDENCE_ERROR: {message}"))),
    };
    match result {
        Ok(result) => (0, render(&result, json), String::new(), export),
        Err(message) => (1, String::new(), format!("{message}\n"), export),
    }
}

/// Whether `--json` was given, and the native operation with its input when enabled.
type Request = (bool, Option<(String, Value)>);

fn request(args: &[String]) -> Result<Request, Failure> {
    let (operation, mut rest) = (args[0].clone(), args[1..].iter());
    let (mut options, mut members, mut flags) = (serde_json::Map::new(), Vec::new(), Vec::new());
    const VALUES: &[&str] = &[
        "--root",
        "--field",
        "--value",
        "--limit",
        "--after",
        "--source",
        "--kind",
        "--under",
        "--max-files",
        "--max-bytes",
        "--max-output-bytes",
        "--max-duration-ms",
    ];
    while let Some(key) = rest.next() {
        let key = key.as_str();
        if matches!(key, "--enable" | "--verify" | "--excerpt" | "--json") {
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
            return Err(format!("unknown or incomplete option: {key}").into());
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
        ("kind", "kind"),
        ("under", "under"),
    ] {
        if let Some(value) = options.get(option) {
            input.insert(field.into(), value.clone());
        }
    }
    input.insert("limit".into(), json!(limit as u64));
    input.insert("verify".into(), json!(flags.contains(&"--verify")));
    input.insert("excerpt".into(), json!(flags.contains(&"--excerpt")));
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

/// Text view: one header per source, one line per row with its enclosing declaration, and
/// match counts to narrow a truncated query.
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
    let shown = crate::render::shown;
    let coverage = &result["coverage"];
    let matches = &result["matches"];
    let mut text = format!(
        "{} result(s){}{}; {}; inventory {}\n",
        rows.len(),
        matches["total"]
            .as_u64()
            .map(|total| format!(" of {total}"))
            .unwrap_or_default(),
        result["kind"]
            .as_str()
            .map(|kind| format!(" of kind {kind}"))
            .unwrap_or_default(),
        shown(&coverage["freshness"]),
        if coverage["inventoryComplete"] == true {
            "complete"
        } else {
            "partial"
        }
    );
    let counts = |value: &Value| {
        value
            .as_object()
            .map(|counts| {
                counts
                    .iter()
                    .map(|(key, count)| format!("{key} {}", shown(count)))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default()
    };
    if matches.is_object() {
        text.push_str(&format!(
            "by kind: {}\nunder {}: {}\n",
            counts(&matches["kinds"]),
            match matches["under"].as_str() {
                Some("") | None => ".",
                Some(under) => under,
            },
            counts(&matches["directories"])
        ));
    }
    let mut current = None;
    for row in rows {
        let path = row["path"].as_str().unwrap_or_default();
        if current != Some(path) {
            current = Some(path);
            let verification = result["sources"][path]["verification"]
                .as_str()
                .unwrap_or("stored-snapshot");
            text.push_str(&format!("{path} [{verification}]\n"));
        }
        text.push_str(&format!(
            "  {}:{} ",
            shown(&row["line"]),
            shown(&row["column"])
        ));
        // A kind filter is stated once in the header.
        if result["kind"].is_null() {
            text.push_str(&format!("{} ", shown(&row["kind"])));
        }
        text.push_str(&shown(&row["name"]));
        if let Some(end) = row["endLine"].as_u64() {
            text.push_str(&format!(" (lines {}-{end})", shown(&row["line"])));
        }
        if let Some(target) = row["target"]
            .as_str()
            .filter(|t| Some(*t) != row["name"].as_str())
        {
            text.push_str(&format!(" -> {target}"));
        }
        let container = &row["container"];
        if container.is_object() {
            text.push_str(&format!(
                " in {} {}:{}",
                shown(&container["kind"]),
                shown(&container["name"]),
                shown(&container["line"])
            ));
        }
        text.push('\n');
        if let Some(excerpt) = row["excerpt"].as_str() {
            text.push_str(&format!("    | {excerpt}\n"));
        }
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
                input(invalid).unwrap_err().message,
                "limit must be between 1 and 200"
            );
        }
        let budget = |value: &str| request(&args(&["refresh", "--enable", "--max-files", value]));
        assert_eq!(
            budget("9007199254740991").unwrap().1.unwrap().1["maxFiles"],
            9_007_199_254_740_991u64
        );
        assert_eq!(
            budget("9007199254740992").unwrap_err().message,
            "max-files must be a positive safe integer"
        );
    }

    #[test]
    fn disabled_access_names_no_operation_and_reads_nothing() {
        let (json, enabled) = request(&args(&["bogus", "--root", "/missing", "--json"])).unwrap();
        assert!(json && enabled.is_none());
        assert_eq!(
            request(&args(&["bogus", "--enable"])).unwrap_err().message,
            "unknown project evidence operation"
        );
        let (code, stdout, stderr, _) = command(&args(&["status"]));
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
            let (code, stdout, stderr, _) = command(&args(list));
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
    fn query_results_render_grouped_by_source_with_containers() {
        let result = json!({"results": [
                {"path": "a.java", "line": 2, "column": 8, "kind": "java:declares-method", "name": "go", "endLine": 4},
                {"path": "a.java", "line": 3, "column": 5, "kind": "java:invokes-syntax", "name": "ping", "target": "ping",
                    "container": {"kind": "java:declares-method", "name": "go", "line": 2}, "excerpt": "ping();"},
                {"path": "b.js", "line": 1, "column": 1, "kind": "js:imports", "name": "x", "target": "./x.js"}],
            "sources": {"a.java": {"verification": "verified-selected-source"}, "b.js": {}},
            "matches": {"total": 9, "kinds": {"java:invokes-syntax": 8, "js:imports": 1}, "under": "", "directories": {"a.java": 8, "b.js": 1}},
            "coverage": {"freshness": "stored-snapshot", "inventoryComplete": false}, "continuation": "next"});
        assert_eq!(
            render(&result, false),
            "3 result(s) of 9; stored-snapshot; inventory partial
by kind: java:invokes-syntax 8, js:imports 1
under .: a.java 8, b.js 1
a.java [verified-selected-source]
  2:8 java:declares-method go (lines 2-4)
  3:5 java:invokes-syntax ping in java:declares-method go:2
    | ping();
b.js [stored-snapshot]
  1:1 js:imports x -> ./x.js
Continue with --after next
"
        );
        assert_eq!(render(&result, true), format!("{result}\n"));
    }
}
