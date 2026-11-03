//! Java syntax relationships only. Method names are not resolved call targets.
use crate::syntax::{Language, Vocabulary};
use craft_engine::Extraction;
use tree_sitter::Node;

pub const PROVIDER: &str = "java-tree-sitter-0.23.5/v3";

pub const LANGUAGE: Language = Language {
    name: "java",
    provider: PROVIDER,
    extensions: &["java"],
    grammar: || tree_sitter_java::LANGUAGE.into(),
    query: || include_str!("../queries/java.scm").into(),
    vocabulary: Vocabulary::Craft,
    kinds: &[
        "java:declares-constructor",
        "java:declares-method",
        "java:declares-type",
        "java:imports",
        "java:imports-static",
        "java:invokes-syntax",
        "java:references-syntax",
    ],
    import: Some(import),
};

/// `import a.b.C;` imports `a.b.C`; `import a.b.*;` imports `a.b.*`.
fn import(node: Node, source: &str) -> Option<String> {
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
    Some(target)
}

pub fn extract(path: &str, bytes: &[u8]) -> Result<Extraction, String> {
    LANGUAGE.extract(path, bytes)
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
        assert!(
            extract("Bad.java", b"class Bad { void broken( }")
                .unwrap()
                .partial
        );
    }

    #[test]
    fn method_references_are_cited_apart_from_calls() {
        let source = b"class A { void f(java.util.List<String> xs) { xs.forEach(Policy::apply); xs.map(this::trim).map(String::new); } }";
        let result = extract("A.java", source).unwrap();
        let references: Vec<_> = result
            .evidence
            .iter()
            .filter(|e| e.kind == "java:references-syntax")
            .map(|e| (e.name.as_str(), &source[e.start_byte..e.end_byte]))
            .collect();
        assert_eq!(
            references,
            [("apply", &b"apply"[..]), ("trim", &b"trim"[..])]
        );
        let owner = result
            .evidence
            .iter()
            .find(|e| e.kind == "java:references-syntax")
            .and_then(|r| {
                result
                    .evidence
                    .iter()
                    .find(|e| Some(&e.id) == r.container.as_ref())
            })
            .unwrap();
        assert_eq!(owner.name, "f");
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
