use craft_engine::Extraction;
use std::path::Path;

pub const ACTIVE: &[&str] = &[
    crate::javascript::PROVIDER,
    crate::java::PROVIDER,
    crate::markdown::PROVIDER,
];

pub fn for_path(path: &str) -> Option<&'static str> {
    match Path::new(path).extension().and_then(|s| s.to_str()) {
        Some("js" | "mjs" | "jsx" | "cjs") => Some(crate::javascript::PROVIDER),
        Some("java") => Some(crate::java::PROVIDER),
        Some("md" | "markdown") => Some(crate::markdown::PROVIDER),
        _ => None,
    }
}

/// The evidence kinds a provider can extract.
pub fn kinds(provider: &str) -> &'static [&'static str] {
    match provider {
        crate::javascript::PROVIDER => &["js:declares", "js:dynamic-import", "js:imports"],
        crate::java::PROVIDER => &[
            "java:declares-constructor",
            "java:declares-method",
            "java:declares-type",
            "java:imports",
            "java:imports-static",
            "java:invokes-syntax",
        ],
        crate::markdown::PROVIDER => &["md:section"],
        _ => &[],
    }
}

pub fn extract(path: &str, bytes: &[u8]) -> Result<Extraction, String> {
    match for_path(path) {
        Some(crate::javascript::PROVIDER) => crate::javascript::extract(path, bytes),
        Some(crate::java::PROVIDER) => crate::java::extract(path, bytes),
        Some(crate::markdown::PROVIDER) => crate::markdown::extract(path, bytes),
        _ => Err("unsupported source extension".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every kind a provider extracts from a sample source is one it declares.
    #[test]
    fn extracted_kinds_are_declared() {
        for (path, source) in [
            (
                "a.mjs",
                "import x from './x.js'; await import('./y.js'); export function f() {}",
            ),
            (
                "A.java",
                "import a.B; import static a.C.d; class A { A() {} void f() { g(); } }",
            ),
            ("a.md", "# Title\n"),
        ] {
            let extraction = extract(path, source.as_bytes()).unwrap();
            let declared = kinds(for_path(path).unwrap());
            for item in &extraction.evidence {
                assert!(declared.contains(&item.kind.as_str()), "{}", item.kind);
            }
        }
    }
}
