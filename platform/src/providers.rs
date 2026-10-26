use craft_engine::Extraction;
use std::path::Path;

pub const ACTIVE: &[&str] = &[crate::javascript::PROVIDER, crate::java::PROVIDER];

pub fn for_path(path: &str) -> Option<&'static str> {
    match Path::new(path).extension().and_then(|s| s.to_str()) {
        Some("js" | "mjs" | "jsx" | "cjs") => Some(crate::javascript::PROVIDER),
        Some("java") => Some(crate::java::PROVIDER),
        _ => None,
    }
}

pub fn extract(path: &str, bytes: &[u8]) -> Result<Extraction, String> {
    match for_path(path) {
        Some(crate::javascript::PROVIDER) => crate::javascript::extract(path, bytes),
        Some(crate::java::PROVIDER) => crate::java::extract(path, bytes),
        _ => Err("unsupported source extension".into()),
    }
}
