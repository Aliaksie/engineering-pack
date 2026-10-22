//! Portable project evidence records. Hashes use raw bytes; IDs use JSON string arrays.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SCHEMA_VERSION: u32 = 1;
pub const PROTOCOL_VERSION: u32 = 1;

pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
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
    /// Literal module specifier, or None for declarations/dynamic imports.
    pub target: Option<String>,
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
            || self.hash.len() != 64
            || !self.hash.bytes().all(|b| b.is_ascii_hexdigit())
            || self.evidence.len() > 20_000
        {
            return Err("invalid extraction binding".into());
        }
        let mut ids = std::collections::HashSet::new();
        for item in &self.evidence {
            if item.line == 0
                || item.column == 0
                || item.start_byte >= item.end_byte
                || item.name.len() > 4096
                || item.kind.is_empty()
                || item.kind.len() > 128
                || item.target.as_ref().is_some_and(|s| s.len() > 4096)
                || item.id
                    != identity(&[
                        &self.path,
                        &self.provider,
                        &item.kind,
                        &item.name,
                        &item.start_byte.to_string(),
                    ])
                || !ids.insert(&item.id)
            {
                return Err("invalid evidence record".into());
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
}
