//! Go syntax relationships only. A call names a function or selector field, not a resolved
//! callee; an import names a package path as written.
use crate::syntax::{Language, Vocabulary};

pub const PROVIDER: &str = "go-tree-sitter-0.25.0/v1";

pub const LANGUAGE: Language = Language {
    name: "go",
    provider: PROVIDER,
    extensions: &["go"],
    grammar: || tree_sitter_go::LANGUAGE.into(),
    query: || include_str!("../queries/go.scm").into(),
    vocabulary: Vocabulary::Craft,
    kinds: &[
        "go:declares-const",
        "go:declares-function",
        "go:declares-method",
        "go:declares-type",
        "go:declares-variable",
        "go:imports",
        "go:invokes-syntax",
    ],
    import: None,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::tests::rows;

    #[test]
    fn functions_methods_types_calls_and_imports() {
        let source = b"package chi\n\nimport (\n\t\"net/http\"\n\tlog \"github.com/x/log\"\n)\n\nconst Limit = 3\nvar Default = New()\n\ntype Router interface {\n\tRoute(p string) Router\n}\n\ntype Mux struct{}\n\nfunc New() *Mux { return &Mux{} }\n\nfunc (m *Mux) Route(p string) Router {\n\tvar local = 1\n\tf := func() { log.Print(p) }\n\tf()\n\treturn RouteContext(http.NewRequest())\n}\n";
        let result = LANGUAGE.extract("mux.go", source).unwrap();
        assert_eq!(
            rows(&result, "declares"),
            [
                ("go:declares-const", "Limit", None),
                ("go:declares-variable", "Default", None),
                ("go:declares-type", "Router", None),
                ("go:declares-method", "Route", None),
                ("go:declares-type", "Mux", None),
                ("go:declares-function", "New", None),
                ("go:declares-method", "Route", None),
            ]
        );
        assert_eq!(
            rows(&result, "invokes"),
            [
                ("go:invokes-syntax", "New", None),
                ("go:invokes-syntax", "Print", Some("Route")),
                ("go:invokes-syntax", "f", Some("Route")),
                ("go:invokes-syntax", "RouteContext", Some("Route")),
                ("go:invokes-syntax", "NewRequest", Some("Route")),
            ]
        );
        assert_eq!(
            rows(&result, "imports"),
            [
                ("go:imports", "net/http", None),
                ("go:imports", "github.com/x/log", None),
            ]
        );
    }
}
