//! Bounded local citation observations, using the scanner's no-follow file open.
use crate::{search, state};
use craft_engine::{contracts, hash, routing};
use serde_json::{Value, json};
use std::{io::Read, path::Path};
pub fn observe(root: &Path, files: &[Value]) -> Result<Value, crate::Error> {
    let root = state::canonical_root(root)?;
    let mut result = Vec::new();
    for reference in files {
        let name = contracts::relative_file(reference)?;
        if routing::private_path(&name) {
            return Err("invalid grounding source path".into());
        }
        let observation = (|| -> Result<String, search::SearchError> {
            let path = search::safe_file(&root, &name)?.ok_or("symlink in context path")?;
            let file = search::open_source(&path)?;
            if file.metadata()?.len() > 1024 * 1024 {
                return Err("grounding read limit".into());
            }
            let mut bytes = Vec::new();
            file.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
            if bytes.len() > 1024 * 1024 {
                return Err("grounding read limit".into());
            }
            Ok(hash(&bytes))
        })();
        result.push(match observation {
            Ok(identity)=>json!({"reference":name,"identity":identity,"status":"observed"}),
            Err(error)=>json!({"reference":name,"identity":null,"status":if ["ENOENT","ENOTDIR"].contains(&error.code()){"missing"}else{"unavailable"}})
        });
    }
    Ok(json!(result))
}
