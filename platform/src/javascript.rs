//! Syntax evidence only: no claims about runtime calls or package resolution.
pub use crate::syntax::MAX_SOURCE_BYTES;
use crate::syntax::{Language, Vocabulary};
use craft_engine::Extraction;

pub const PROVIDER: &str = "javascript-tree-sitter-0.25.0/v3";

pub const LANGUAGE: Language = Language {
    name: "js",
    provider: PROVIDER,
    extensions: &["js", "mjs", "jsx", "cjs"],
    grammar: || tree_sitter_javascript::LANGUAGE.into(),
    query: || include_str!("../queries/javascript.scm").into(),
    vocabulary: Vocabulary::Craft,
    import: None,
};

pub fn extract(path: &str, bytes: &[u8]) -> Result<Extraction, String> {
    LANGUAGE.extract(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn syntax_is_cited_without_inventing_dynamic_targets() {
        let result = extract(
            "src/a.mjs",
            b"import {x} from './b.mjs';\nexport function f() { return import(name); }",
        )
        .unwrap();
        assert_eq!(result.evidence.len(), 3);
        assert_eq!(result.evidence[0].target.as_deref(), Some("./b.mjs"));
        assert_eq!(result.evidence[1].name, "f");
        assert_eq!(result.evidence[1].line, 2);
        assert_eq!(result.evidence[2].kind, "js:dynamic-import");
        assert_eq!(result.evidence[2].target, None);
        assert!(extract("bad.js", b"export function {").is_err());
        assert!(extract("bad.js", &[255]).is_err());
        assert!(
            extract("empty.js", b"// empty")
                .unwrap()
                .evidence
                .is_empty()
        );
    }
}
