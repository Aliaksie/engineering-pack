//! Python syntax relationships only. A call names an attribute or identifier, not a resolved
//! callee, and an import names a module as written.
use crate::syntax::{Language, Vocabulary};

pub const PROVIDER: &str = "python-tree-sitter-0.25.0/v1";

pub const LANGUAGE: Language = Language {
    name: "python",
    provider: PROVIDER,
    extensions: &["py", "pyi"],
    grammar: || tree_sitter_python::LANGUAGE.into(),
    query: || include_str!("../queries/python.scm").into(),
    vocabulary: Vocabulary::Craft,
    kinds: &[
        "python:declares-class",
        "python:declares-function",
        "python:declares-method",
        "python:declares-variable",
        "python:imports",
        "python:imports-name",
        "python:invokes-syntax",
    ],
    import: None,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::tests::rows;

    #[test]
    fn methods_functions_calls_and_imports() {
        let source = b"import os, a.b as c\nfrom .x import y\nLIMIT = 3\n\nclass Client:\n    retries = setup()\n\n    @property\n    def timeout(self):\n        return self.load()\n\n    def send(self):\n        def inner():\n            helper()\n        return Timeout(inner())\n\ndef main():\n    Client().send()\n";
        let result = LANGUAGE.extract("client.py", source).unwrap();
        assert_eq!(
            rows(&result, "declares"),
            [
                ("python:declares-variable", "LIMIT", None),
                ("python:declares-class", "Client", None),
                ("python:declares-method", "timeout", None),
                ("python:declares-method", "send", None),
                ("python:declares-function", "inner", None),
                ("python:declares-function", "main", None),
            ]
        );
        assert_eq!(
            rows(&result, "invokes"),
            [
                ("python:invokes-syntax", "setup", None),
                ("python:invokes-syntax", "load", Some("timeout")),
                ("python:invokes-syntax", "helper", Some("inner")),
                ("python:invokes-syntax", "Timeout", Some("send")),
                ("python:invokes-syntax", "inner", Some("send")),
                ("python:invokes-syntax", "send", Some("main")),
                ("python:invokes-syntax", "Client", Some("main")),
            ]
        );
        assert_eq!(
            rows(&result, "imports"),
            [
                ("python:imports", "os", None),
                ("python:imports", "a.b", None),
                ("python:imports", ".x", None),
                ("python:imports-name", "y", None),
            ]
        );
    }
}
