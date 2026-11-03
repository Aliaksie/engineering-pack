//! Rust syntax relationships only. Call names are not resolved targets, and macro arguments are
//! token trees, so a call there is recognized only by its `name(` spelling.
use crate::syntax::{Language, Vocabulary};
use craft_engine::Extraction;
use tree_sitter::Node;

pub const PROVIDER: &str = "rust-tree-sitter-0.24.2/v3";

pub const LANGUAGE: Language = Language {
    name: "rust",
    provider: PROVIDER,
    extensions: &["rs"],
    grammar: || tree_sitter_rust::LANGUAGE.into(),
    query: || include_str!("../queries/rust.scm").into(),
    vocabulary: Vocabulary::Craft,
    kinds: &[
        "rust:declares-const",
        "rust:declares-fn",
        "rust:declares-macro",
        "rust:declares-module",
        "rust:declares-type",
        "rust:imports",
        "rust:invokes-syntax",
        "rust:references-syntax",
    ],
    import: Some(import),
};

fn text(node: Node, source: &str) -> String {
    source[node.byte_range()]
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}

/// One imported path per `use` tree leaf: `use a::{b, c as d, e::*}` imports `a::b`, `a::c`
/// and `a::e::*`.
fn import(node: Node, source: &str) -> Option<String> {
    let parent = node.parent()?;
    let leaf = match parent.kind() {
        "use_declaration" => parent.child_by_field_name("argument") == Some(node),
        "use_list" => node.is_named() && !node.is_extra(),
        _ => false,
    };
    if !leaf || matches!(node.kind(), "use_list" | "scoped_use_list") {
        return None;
    }
    let leaf = match node.kind() {
        "use_as_clause" => text(node.child_by_field_name("path")?, source),
        _ => text(node, source),
    };
    let mut prefixes = Vec::new();
    let mut ancestor = parent;
    loop {
        match ancestor.kind() {
            "use_declaration" => break,
            "use_list" => {}
            "scoped_use_list" => {
                if let Some(path) = ancestor.child_by_field_name("path") {
                    prefixes.push(text(path, source));
                }
            }
            _ => return None,
        }
        ancestor = ancestor.parent()?;
    }
    prefixes.reverse();
    if leaf != "self" || prefixes.is_empty() {
        prefixes.push(leaf);
    }
    Some(prefixes.join("::"))
}

pub fn extract(path: &str, bytes: &[u8]) -> Result<Extraction, String> {
    LANGUAGE.extract(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows<'a>(result: &'a Extraction, kind: &str) -> Vec<(&'a str, Option<&'a str>)> {
        result
            .evidence
            .iter()
            .filter(|e| e.kind == kind)
            .map(|e| {
                let owner = result
                    .evidence
                    .iter()
                    .find(|o| Some(&o.id) == e.container.as_ref());
                (e.name.as_str(), owner.map(|o| o.name.as_str()))
            })
            .collect()
    }

    #[test]
    fn function_paths_used_as_values_are_references_not_calls() {
        let source = b"fn run() {\n    let a = x.and_then(prompt::request);\n    let f = self::plan;\n    let s = S { h: a::handle, k: Kind::Callers };\n    go(MAX::LIMIT, crate::Ty, &b::check);\n    prompt::request(x);\n}\n";
        let result = extract("lib.rs", source).unwrap();
        assert_eq!(
            rows(&result, "rust:references-syntax"),
            [
                ("request", Some("run")),
                ("plan", Some("run")),
                ("handle", Some("run")),
                ("check", Some("run"))
            ]
        );
        assert_eq!(
            rows(&result, "rust:invokes-syntax"),
            [
                ("and_then", Some("run")),
                ("go", Some("run")),
                ("request", Some("run"))
            ]
        );
    }

    #[test]
    fn use_trees_import_one_path_per_leaf() {
        let source = b"use std::io::{self, Read as R};\npub use crate::{a::{b, c::*}, d};\nuse e;\nuse ::f::g;\n";
        let result = extract("lib.rs", source).unwrap();
        let imports: Vec<_> = result
            .evidence
            .iter()
            .filter(|e| e.kind == "rust:imports")
            .map(|e| e.target.as_deref().unwrap())
            .collect();
        assert_eq!(
            imports,
            [
                "std::io",
                "std::io::Read",
                "crate::a::b",
                "crate::a::c::*",
                "crate::d",
                "e",
                "::f::g"
            ]
        );
    }

    #[test]
    fn calls_name_their_token_and_nearest_function() {
        let source = b"struct S;\nimpl S {\n    fn new() -> Self { helper(); S }\n    fn run(&self) { let f = || Self::new(); self.go::<u8>(f()); }\n}\nfn outer() {\n    fn inner() { x.y.z(); }\n    impl T { const C: u8 = konst(); }\n}\nconst TOP: u8 = top();\n";
        let result = extract("s.rs", source).unwrap();
        assert_eq!(
            rows(&result, "rust:invokes-syntax"),
            [
                ("helper", Some("new")),
                ("new", Some("run")),
                ("go", Some("run")),
                ("f", Some("run")),
                ("z", Some("inner")),
                ("konst", None),
                ("top", None)
            ]
        );
        for call in result
            .evidence
            .iter()
            .filter(|e| e.kind == "rust:invokes-syntax")
        {
            assert_eq!(
                &source[call.start_byte..call.end_byte],
                call.name.as_bytes()
            );
        }
        let declared: Vec<_> = result
            .evidence
            .iter()
            .filter(|e| e.kind.starts_with("rust:declares"))
            .map(|e| (e.kind.as_str(), e.name.as_str()))
            .collect();
        assert_eq!(
            declared,
            [
                ("rust:declares-type", "S"),
                ("rust:declares-fn", "new"),
                ("rust:declares-fn", "run"),
                ("rust:declares-fn", "outer"),
                ("rust:declares-fn", "inner"),
                ("rust:declares-const", "C"),
                ("rust:declares-const", "TOP")
            ]
        );
    }

    #[test]
    fn macro_arguments_yield_spelled_calls_but_not_attributes_or_definitions() {
        let source = b"#[cfg(all(unix, not(test)))]\nfn f() {\n    assert_eq!(parse(x).len(), 1, \"no(call)\");\n    let v = vec![a::make(1)];\n    if let Some(a) = g() && let Ok(b) = h(a) {}\n}\nmacro_rules! m { ($e:expr) => { hidden($e) }; }\n";
        let result = extract("m.rs", source).unwrap();
        assert_eq!(
            rows(&result, "rust:invokes-syntax"),
            [
                ("parse", Some("f")),
                ("len", Some("f")),
                ("make", Some("f")),
                ("g", Some("f")),
                ("h", Some("f"))
            ]
        );
        assert!(
            result
                .evidence
                .iter()
                .any(|e| e.kind == "rust:declares-macro" && e.name == "m")
        );
        assert!(extract("bad.rs", b"fn broken( {}").unwrap().partial);
    }
}
