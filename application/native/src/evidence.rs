use craft_engine::hash;
use craft_platform::{
    javascript::{PROVIDER, extract},
    repository::{Inventory, database_guard, eligible, now, read_source},
    store::Store,
};
use serde_json::{Value, json};
use std::{path::Path, time::Instant};

pub fn run(operation: &str, input: &Value) -> Result<Value, String> {
    let root = Path::new(input["root"].as_str().ok_or("root is required")?)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let root_text = root
        .to_str()
        .ok_or("non-UTF-8 repository root unsupported")?;
    let expected = root
        .join(".craft")
        .join("state")
        .join(&hash(root_text.as_bytes())[..24])
        .join("shared")
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
        "query" => {
            let mut result = store.query(
                input["field"].as_str().unwrap_or("name"),
                input["value"].as_str().ok_or("query value required")?,
                input["after"].as_str().unwrap_or(""),
                input["limit"].as_u64().unwrap_or(30) as usize,
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
            let started = Instant::now();
            let retired = store.retain_providers(&[PROVIDER])?;
            let epoch = store.begin_inventory()?;
            let mut inventory = Inventory::start(&root)?;
            let (mut files, mut reused, mut changed, mut failed, mut bytes) =
                (0u64, 0u64, 0u64, 0u64, 0u64);
            while let Some(path) = inventory.next_path()? {
                if !eligible(&path) {
                    continue;
                }
                // Git's tracked inventory includes deleted files; completion retires them.
                if !root.join(&path).try_exists().map_err(|e| e.to_string())? {
                    continue;
                }
                files += 1;
                let observed_at = now();
                let content = match read_source(&root, &path) {
                    Ok(content) => content,
                    Err(error) => {
                        store.fail(&path, PROVIDER, "", &error, &observed_at, epoch)?;
                        failed += 1;
                        continue;
                    }
                };
                bytes += content.len() as u64;
                let digest = hash(&content);
                if store.reuse(&path, PROVIDER, &digest, epoch)? {
                    reused += 1;
                    continue;
                }
                match extract(&path, &content) {
                    Ok(extraction) => {
                        store.replace(&extraction, &observed_at, epoch)?;
                        changed += 1;
                    }
                    Err(error) => {
                        store.fail(&path, PROVIDER, &digest, &error, &observed_at, epoch)?;
                        failed += 1;
                    }
                }
            }
            let removed = store.finish_inventory(PROVIDER, epoch)?;
            Ok(
                json!({"coverage":store.status()?,"files":files,"reused":reused,"changed":changed,"failed":failed,"removed":removed,
                "retiredSources":retired,"bytesScanned":bytes,"wallTimeMs":started.elapsed().as_millis(),"scope":root}),
            )
        }
        _ => Err("unknown evidence operation".into()),
    }
}
