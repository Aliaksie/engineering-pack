//! Project Intelligence providers: which provider reads a source, the evidence kinds it can
//! extract and the extraction itself. Programming languages go through the one query-driven
//! syntax extractor; Markdown and configuration files have scanners of their own.

pub mod basic;
pub mod config;
pub mod go;
pub mod java;
pub mod javascript;
pub mod markdown;
pub mod python;
pub mod rust;
pub(crate) mod syntax;
pub mod typescript;

use craft_engine::Extraction;
use std::path::Path;
use std::sync::LazyLock;
use syntax::Language;

/// Languages read by the common syntax extractor.
pub const LANGUAGES: &[&Language] = &[
    &javascript::LANGUAGE,
    &java::LANGUAGE,
    &rust::LANGUAGE,
    &typescript::LANGUAGE,
    &typescript::TSX,
    &python::LANGUAGE,
    &go::LANGUAGE,
    &basic::C,
    &basic::CPP,
    &basic::CSHARP,
    &basic::PHP,
    &basic::RUBY,
];

/// Every provider this binary extracts with.
pub static ACTIVE: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    let mut active: Vec<_> = LANGUAGES.iter().map(|l| l.provider).collect();
    active.push(markdown::PROVIDER);
    active.push(config::PROVIDER);
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
        Some("md" | "markdown") => Some(markdown::PROVIDER),
        Some("json" | "jsonc" | "toml" | "yaml" | "yml") => Some(config::PROVIDER),
        _ => language(path).map(|l| l.provider),
    }
}

/// Whether a source of `bytes` at `path` is data its provider does not index (a lockfile or
/// an oversized configuration file), so a refresh leaves it unread.
pub fn data(path: &str, bytes: u64) -> bool {
    for_path(path) == Some(config::PROVIDER) && config::data(path, bytes)
}

/// The evidence kinds a provider can extract.
pub fn kinds(provider: &str) -> &'static [&'static str] {
    if provider == markdown::PROVIDER {
        return markdown::KINDS;
    }
    if provider == config::PROVIDER {
        return &[config::KIND];
    }
    LANGUAGES
        .iter()
        .find(|l| l.provider == provider)
        .map_or(&[], |l| l.kinds)
}

pub fn extract(path: &str, bytes: &[u8]) -> Result<Extraction, String> {
    match for_path(path) {
        Some(markdown::PROVIDER) => markdown::extract(path, bytes),
        Some(config::PROVIDER) => config::extract(path, bytes),
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
            ("a.md", "# Title\n[x](b.md) `c` "),
            ("a.json", "{\"a\": {\"b\": 1}}"),
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

#[cfg(test)]
mod declared {
    /// A language's declared kinds are exactly what its query can produce, and its queries
    /// compile.
    #[test]
    fn declared_kinds_match_query_captures() {
        for language in super::LANGUAGES {
            let mut declared: Vec<String> = language.kinds.iter().map(|k| k.to_string()).collect();
            declared.sort();
            assert_eq!(declared, language.query_kinds(), "{}", language.provider);
        }
    }

    /// In every language a call or reference token rests on its name and a declaration or
    /// import on the parse; the Markdown and configuration kinds likewise.
    #[test]
    fn every_kind_has_the_basis_of_its_role() {
        for language in super::LANGUAGES {
            for (kind, invokes) in language.kind_roles() {
                let expected = if invokes { "name" } else { "parsed" };
                assert_eq!(craft_engine::basis(&kind), expected, "{kind}");
            }
        }
        for kind in super::markdown::KINDS {
            let expected = if *kind == "md:mention" {
                "name"
            } else {
                "parsed"
            };
            assert_eq!(craft_engine::basis(kind), expected, "{kind}");
        }
        assert_eq!(craft_engine::basis(super::config::KIND), "parsed");
    }
}
