use crate::syntax::Language;
use craft_engine::Extraction;
use std::path::Path;
use std::sync::LazyLock;

/// Languages read by the common syntax extractor.
pub const LANGUAGES: &[&Language] = &[
    &crate::javascript::LANGUAGE,
    &crate::java::LANGUAGE,
    &crate::rust::LANGUAGE,
];

/// Every provider this binary extracts with.
pub static ACTIVE: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    let mut active: Vec<_> = LANGUAGES.iter().map(|l| l.provider).collect();
    active.push(crate::markdown::PROVIDER);
    active
});

fn language(path: &str) -> Option<&'static Language> {
    let extension = Path::new(path).extension()?.to_str()?;
    LANGUAGES
        .iter()
        .copied()
        .find(|l| l.extensions.contains(&extension))
}

pub fn for_path(path: &str) -> Option<&'static str> {
    match Path::new(path).extension().and_then(|s| s.to_str()) {
        Some("md" | "markdown") => Some(crate::markdown::PROVIDER),
        _ => language(path).map(|l| l.provider),
    }
}

/// The evidence kinds a provider can extract.
pub fn kinds(provider: &str) -> &'static [&'static str] {
    static KINDS: LazyLock<Vec<(&'static str, Vec<&'static str>)>> = LazyLock::new(|| {
        let mut kinds: Vec<_> = LANGUAGES
            .iter()
            .map(|l| {
                let kinds = l.kinds().into_iter().map(|k| &*k.leak()).collect();
                (l.provider, kinds)
            })
            .collect();
        kinds.push((crate::markdown::PROVIDER, vec!["md:section"]));
        kinds
    });
    KINDS
        .iter()
        .find(|(p, _)| *p == provider)
        .map_or(&[], |(_, kinds)| kinds.as_slice())
}

pub fn extract(path: &str, bytes: &[u8]) -> Result<Extraction, String> {
    match for_path(path) {
        Some(crate::markdown::PROVIDER) => crate::markdown::extract(path, bytes),
        Some(_) => language(path)
            .ok_or("unsupported source extension")?
            .extract(path, bytes),
        None => Err("unsupported source extension".into()),
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
                "import a.B; import static a.C.d; class A { A() {} void f() { g(this::f); } }",
            ),
            ("a.md", "# Title\n"),
            (
                "a.rs",
                "use a::{b, c}; mod m; macro_rules! n { () => {} } const C: u8 = 1; struct S; fn f() { g(a::f); n!(h()); }",
            ),
        ] {
            let extraction = extract(path, source.as_bytes()).unwrap();
            let declared = kinds(for_path(path).unwrap());
            assert!(!extraction.evidence.is_empty(), "{path}");
            for item in &extraction.evidence {
                assert!(declared.contains(&item.kind.as_str()), "{}", item.kind);
            }
        }
    }
}
