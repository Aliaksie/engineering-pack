//! Rust syntax relationships only. Call names are not resolved targets, and macro arguments are
//! token trees, so a call there is recognized only by its `name(` spelling.
use crate::syntax::{self, Anchor, Fact};
use craft_engine::{Extraction, identity};
use tree_sitter::Node;

pub const PROVIDER: &str = "rust-tree-sitter-0.24.2/v1";

fn declaration(kind: &str) -> Option<&'static str> {
    match kind {
        "function_item" | "function_signature_item" => Some("rust:declares-fn"),
        "struct_item" | "enum_item" | "union_item" | "trait_item" | "type_item" => {
            Some("rust:declares-type")
        }
        "const_item" | "static_item" => Some("rust:declares-const"),
        "mod_item" => Some("rust:declares-module"),
        "macro_definition" => Some("rust:declares-macro"),
        _ => None,
    }
}

fn text(node: Node, source: &str) -> String {
    source[node.byte_range()]
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}

fn container(path: &str, node: Node, source: &str) -> Option<String> {
    let mut parent = node.parent();
    while let Some(node) = parent {
        // An item body inside a function (impl, trait, module) does not run as that function.
        if node.kind() == "declaration_list" {
            return None;
        }
        if node.kind() == "function_item" {
            let name = node.child_by_field_name("name")?;
            return Some(identity(&[
                path,
                PROVIDER,
                "rust:declares-fn",
                &source[name.byte_range()],
                &node.start_byte().to_string(),
            ]));
        }
        parent = node.parent();
    }
    None
}

/// The name token of a called expression: `f`, `a::f`, `x.f` or `f::<T>`.
fn callee(node: Node) -> Option<Node> {
    match node.kind() {
        "identifier" => Some(node),
        "scoped_identifier" => node.child_by_field_name("name"),
        "field_expression" => node
            .child_by_field_name("field")
            .filter(|n| n.kind() == "field_identifier"),
        "generic_function" => callee(node.child_by_field_name("function")?),
        _ => None,
    }
}

/// `name(` inside a macro invocation's arguments, outside attributes and macro definitions.
fn token_call(node: Node, source: &str) -> bool {
    let Some(next) = node.next_sibling() else {
        return false;
    };
    if node.kind() != "identifier"
        || node.parent().is_none_or(|p| p.kind() != "token_tree")
        || next.kind() != "token_tree"
        || !source[next.byte_range()].starts_with('(')
        || node.prev_sibling().is_some_and(|p| p.kind() == "fn")
    {
        return false;
    }
    let mut parent = node.parent();
    while let Some(node) = parent {
        match node.kind() {
            "attribute" | "macro_definition" => return false,
            "macro_invocation" => return true,
            _ => parent = node.parent(),
        }
    }
    false
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
    syntax::extract(
        path,
        bytes,
        PROVIDER,
        tree_sitter_rust::LANGUAGE.into(),
        |node, source| {
            if let Some(kind) = declaration(node.kind()) {
                let name = node.child_by_field_name("name")?;
                return Some(Fact {
                    kind,
                    name: source[name.byte_range()].into(),
                    target: None,
                    container: None,
                    anchor: None,
                });
            }
            let name = if node.kind() == "call_expression" {
                Some(callee(node.child_by_field_name("function")?)?)
            } else if token_call(node, source) {
                Some(node)
            } else {
                None
            };
            if let Some(name) = name {
                let anchor = Anchor {
                    range: name.byte_range(),
                    position: name.start_position(),
                };
                let name = source[name.byte_range()].to_string();
                return Some(Fact {
                    kind: "rust:invokes-syntax",
                    target: Some(name.clone()),
                    name,
                    container: container(path, node, source),
                    anchor: Some(anchor),
                });
            }
            let target = import(node, source)?;
            Some(Fact {
                kind: "rust:imports",
                name: target.clone(),
                target: Some(target),
                container: None,
                anchor: None,
            })
        },
    )
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
        assert!(extract("bad.rs", b"fn broken( {}").is_err());
    }
}
