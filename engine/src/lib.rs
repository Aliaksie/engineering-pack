//! Portable project evidence records. Hashes use raw bytes; IDs use JSON string arrays.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub mod actions;
pub mod checkpoint;
pub mod context;
pub mod contracts;
pub mod control;
pub mod delivery;
pub mod discovery;
pub mod grounding;
pub mod models;
pub mod otlp;
pub mod repository;
pub mod retention;
pub mod review;
pub mod routing;
pub mod scratch;
pub mod services;
pub mod session;
pub mod telemetry;
pub mod timestamp;
pub mod values;
pub mod workers;

pub const SCHEMA_VERSION: u32 = 1;
pub const PROTOCOL_VERSION: u32 = 1;

pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// SHA-256 of the JSON text, as `JSON.stringify` writes it for normalized values.
pub fn digest(value: &serde_json::Value) -> String {
    hash(&serde_json::to_vec(value).expect("JSON values serialize"))
}

/// A copy with object keys sorted by UTF-16 code units, as `canonical` in the JS engine.
pub fn canonical(value: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
        Value::Object(map) => {
            let mut keys: Vec<_> = map.keys().collect();
            keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
            Value::Object(
                keys.into_iter()
                    .map(|k| (k.clone(), canonical(&map[k])))
                    .collect(),
            )
        }
        other => other.clone(),
    }
}

pub fn identity(parts: &[&str]) -> String {
    hash(&serde_json::to_vec(parts).expect("string arrays serialize"))
}

pub fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 4096
        && !path.contains(['\\', '\0', ':'])
        && path
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Evidence {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub line: usize,
    pub column: usize,
    pub start_byte: usize,
    pub end_byte: usize,
    /// Provider-defined syntax target; not a resolved runtime relationship.
    pub target: Option<String>,
    /// Same-source lexical declaration ID; not a resolved call target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Extraction {
    pub path: String,
    pub hash: String,
    pub provider: String,
    pub evidence: Vec<Evidence>,
}

impl Extraction {
    pub fn validate(&self) -> Result<(), String> {
        if !valid_path(&self.path)
            || self.provider.is_empty()
            || self.provider.len() > 256
            || !values::lower_hex(&self.hash, 64)
            || self.evidence.len() > 20_000
        {
            return Err("invalid extraction binding".into());
        }
        let mut ids = std::collections::HashMap::new();
        for item in &self.evidence {
            if item.line == 0
                || item.column == 0
                || item.start_byte >= item.end_byte
                || item.name.len() > 4096
                || item.kind.is_empty()
                || item.kind.len() > 128
                || item.target.as_ref().is_some_and(|s| s.len() > 4096)
                || item
                    .container
                    .as_ref()
                    .is_some_and(|s| !values::lower_hex(s, 64) || s == &item.id)
                || item.id
                    != identity(&[
                        &self.path,
                        &self.provider,
                        &item.kind,
                        &item.name,
                        &item.start_byte.to_string(),
                    ])
                || ids.insert(&item.id, item).is_some()
            {
                return Err("invalid evidence record".into());
            }
        }
        for item in &self.evidence {
            if let Some(container) = &item.container {
                let owner = ids.get(container).ok_or("missing evidence container")?;
                if owner.start_byte > item.start_byte || owner.end_byte < item.end_byte {
                    return Err("evidence container does not enclose source range".into());
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identities_are_framed_and_replayable() {
        assert_eq!(
            hash(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(identity(&["a", "bc"]), hash(br#"["a","bc"]"#));
        assert_ne!(identity(&["a", "bc"]), identity(&["ab", "c"]));
        for path in ["/tmp/a", "../a", "a/../b", "a\\b", "C:/a", "a//b"] {
            assert!(!valid_path(path));
        }
        assert!(valid_path("src/hello world.mjs"));
    }

    #[test]
    fn extraction_hashes_are_lowercase_as_stored() {
        let id = identity(&["a.js", "p", "function", "f", "0"]);
        let evidence = |container: Option<String>| Evidence {
            id: id.clone(),
            kind: "function".into(),
            name: "f".into(),
            line: 1,
            column: 1,
            start_byte: 0,
            end_byte: 1,
            target: None,
            container,
        };
        let extraction = |hash: String, container| Extraction {
            path: "a.js".into(),
            hash,
            provider: "p".into(),
            evidence: vec![evidence(container)],
        };
        let digest = hash(b"a");
        assert!(extraction(digest.clone(), None).validate().is_ok());
        // Reuse compares the stored hash byte for byte, so an uppercase binding never matches.
        assert!(extraction(digest.to_uppercase(), None).validate().is_err());
        assert!(
            extraction(digest.clone(), Some(digest.to_uppercase()))
                .validate()
                .is_err()
        );
    }
}
