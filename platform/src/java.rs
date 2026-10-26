//! Java syntax relationships only. Method names are not resolved call targets.
use crate::syntax::{self, Anchor, Fact};
use craft_engine::{Extraction, identity};
use tree_sitter::Node;

pub const PROVIDER: &str = "java-tree-sitter-0.23.5/v1";

fn declaration(kind: &str) -> Option<&'static str> {
    match kind {
        "class_declaration"
        | "interface_declaration"
        | "enum_declaration"
        | "record_declaration"
        | "annotation_type_declaration" => Some("java:declares-type"),
        "method_declaration" => Some("java:declares-method"),
        "constructor_declaration" | "compact_constructor_declaration" => {
            Some("java:declares-constructor")
        }
        _ => None,
    }
}

fn container(path: &str, node: Node, source: &str) -> Option<String> {
    let mut parent = node.parent();
    while let Some(node) = parent {
        // A local/anonymous type's initializer is not an invocation by the outer method.
        if matches!(
            node.kind(),
            "class_body" | "enum_body" | "interface_body" | "annotation_type_body"
        ) {
            return None;
        }
        if matches!(
            node.kind(),
            "method_declaration" | "constructor_declaration" | "compact_constructor_declaration"
        ) {
            let name = node.child_by_field_name("name")?;
            return Some(identity(&[
                path,
                PROVIDER,
                declaration(node.kind())?,
                &source[name.byte_range()],
                &node.start_byte().to_string(),
            ]));
        }
        parent = node.parent();
    }
    None
}

pub fn extract(path: &str, bytes: &[u8]) -> Result<Extraction, String> {
    syntax::extract(
        path,
        bytes,
        PROVIDER,
        tree_sitter_java::LANGUAGE.into(),
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
            if node.kind() == "method_invocation" {
                let name = node.child_by_field_name("name")?;
                let anchor = Anchor {
                    range: name.byte_range(),
                    position: name.start_position(),
                };
                let name = source[name.byte_range()].to_string();
                return Some(Fact {
                    kind: "java:invokes-syntax",
                    target: Some(name.clone()),
                    name,
                    container: container(path, node, source),
                    anchor: Some(anchor),
                });
            }
            if node.kind() == "import_declaration" {
                let mut cursor = node.walk();
                let target = node
                    .named_children(&mut cursor)
                    .find(|n| matches!(n.kind(), "identifier" | "scoped_identifier"))?;
                let mut target = source[target.byte_range()].to_string();
                if node
                    .named_children(&mut cursor)
                    .any(|n| n.kind() == "asterisk")
                {
                    target.push_str(".*");
                }
                let kind = if node.children(&mut cursor).any(|n| n.kind() == "static") {
                    "java:imports-static"
                } else {
                    "java:imports"
                };
                return Some(Fact {
                    kind,
                    name: target.clone(),
                    target: Some(target),
                    container: None,
                    anchor: None,
                });
            }
            None
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chained_same_name_calls_have_distinct_name_token_citations() {
        let source = b"class A { String f(String s) { return s.trim().trim(); } }";
        let result = extract("A.java", source).unwrap();
        let calls: Vec<_> = result
            .evidence
            .iter()
            .filter(|e| e.kind == "java:invokes-syntax")
            .collect();
        assert_eq!(calls.len(), 2);
        assert_ne!(calls[0].id, calls[1].id);
        for call in calls {
            assert_eq!(&source[call.start_byte..call.end_byte], b"trim");
        }
    }

    #[test]
    fn calls_have_lexical_containers_not_resolved_callees() {
        let source = b"import example.Schema;\nimport static java.util.Collections.*;\nclass Generator {\n String type(Schema s) { return schema(s); }\n String type(String s) { return super.type(s); }\n String schema(Schema s) { return \"notACall()\"; }\n}";
        let result = extract("Generator.java", source).unwrap();
        let calls: Vec<_> = result
            .evidence
            .iter()
            .filter(|e| e.kind == "java:invokes-syntax")
            .collect();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "schema");
        assert_eq!(calls[0].target.as_deref(), Some("schema"));
        assert_ne!(calls[0].container, calls[1].container);
        for call in calls {
            let owner = result
                .evidence
                .iter()
                .find(|e| Some(&e.id) == call.container.as_ref())
                .unwrap();
            assert_eq!(owner.kind, "java:declares-method");
            assert_eq!(owner.name, "type");
            assert_eq!(owner.line, call.line);
        }
        assert!(
            result
                .evidence
                .iter()
                .any(|e| e.kind == "java:imports-static"
                    && e.target.as_deref() == Some("java.util.Collections.*"))
        );
        assert!(extract("Bad.java", b"class Bad { void broken( }").is_err());
    }

    #[test]
    fn constructors_lambdas_and_anonymous_initializers_keep_distinct_containers() {
        let source = b"class A { int field = init(); A() { setup(); } void run() { Runnable r = () -> later(); Object o = new Object() { int x = init(); void nested() { inner(); } }; } }";
        let result = extract("A.java", source).unwrap();
        let owners: Vec<_> = result
            .evidence
            .iter()
            .filter(|e| e.kind == "java:invokes-syntax")
            .map(|call| {
                (
                    call.name.as_str(),
                    result
                        .evidence
                        .iter()
                        .find(|e| Some(&e.id) == call.container.as_ref())
                        .map(|e| e.name.as_str()),
                )
            })
            .collect();
        assert_eq!(
            owners,
            vec![
                ("init", None),
                ("setup", Some("A")),
                ("later", Some("run")),
                ("init", None),
                ("inner", Some("nested"))
            ]
        );
    }
}
