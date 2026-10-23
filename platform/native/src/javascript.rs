//! Syntax evidence only: no claims about runtime calls or package resolution.
pub use crate::syntax::MAX_SOURCE_BYTES;
use crate::syntax::{self, Fact};
use craft_engine::Extraction;

pub const PROVIDER: &str = "javascript-tree-sitter-0.25.0/v2";

pub fn extract(path: &str, bytes: &[u8]) -> Result<Extraction, String> {
    syntax::extract(
        path,
        bytes,
        PROVIDER,
        tree_sitter_javascript::LANGUAGE.into(),
        |node, source| {
            let kind = node.kind();
            let declaration = matches!(
                kind,
                "function_declaration"
                    | "generator_function_declaration"
                    | "class_declaration"
                    | "variable_declarator"
            );
            let import = kind == "import_statement"
                || kind == "export_statement" && node.child_by_field_name("source").is_some();
            let dynamic = kind == "call_expression"
                && node
                    .child_by_field_name("function")
                    .is_some_and(|n| n.kind() == "import");
            let name_node = if declaration {
                node.child_by_field_name("name")
            } else {
                None
            };
            if import || dynamic || name_node.is_some_and(|n| n.kind() == "identifier") {
                let target = node.child_by_field_name("source").and_then(|n| {
                    let raw = &source[n.byte_range()];
                    // Escapes need JS string interpretation; retain them as unresolved.
                    (!raw.contains('\\') && raw.len() >= 2)
                        .then(|| raw[1..raw.len() - 1].to_string())
                });
                let name = name_node
                    .map(|n| source[n.byte_range()].to_string())
                    .or_else(|| target.clone())
                    .unwrap_or_else(|| "unresolved import".into());
                let predicate = if dynamic {
                    "js:dynamic-import"
                } else if import {
                    "js:imports"
                } else {
                    "js:declares"
                };
                Some(Fact {
                    kind: predicate,
                    name,
                    target,
                    container: None,
                    anchor: None,
                })
            } else {
                None
            }
        },
    )
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
