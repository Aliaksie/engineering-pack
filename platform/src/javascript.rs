//! Syntax evidence only: no claims about runtime calls or package resolution.
pub use crate::syntax::MAX_SOURCE_BYTES;
use crate::syntax::{Language, Vocabulary};
use craft_engine::Extraction;

pub const PROVIDER: &str = "javascript-tree-sitter-0.25.0/v4";

pub const LANGUAGE: Language = Language {
    name: "js",
    provider: PROVIDER,
    extensions: &["js", "mjs", "jsx", "cjs"],
    grammar: || tree_sitter_javascript::LANGUAGE.into(),
    query: || {
        [
            include_str!("../queries/ecmascript.scm"),
            include_str!("../queries/javascript.scm"),
        ]
        .join("\n")
    },
    vocabulary: Vocabulary::Craft,
    kinds: &[
        "js:declares-class",
        "js:declares-function",
        "js:declares-method",
        "js:declares-variable",
        "js:dynamic-import",
        "js:imports",
        "js:imports-name",
        "js:invokes-syntax",
    ],
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
        assert_eq!(result.evidence.len(), 4);
        assert_eq!(result.evidence[0].target.as_deref(), Some("./b.mjs"));
        assert_eq!(result.evidence[1].kind, "js:imports-name");
        assert_eq!(result.evidence[1].name, "x");
        assert_eq!(result.evidence[2].name, "f");
        assert_eq!(result.evidence[2].line, 2);
        assert_eq!(result.evidence[3].kind, "js:dynamic-import");
        assert_eq!(result.evidence[3].target, None);
        assert!(extract("bad.js", b"export function {").unwrap().partial);
        assert!(extract("bad.js", &[255]).is_err());
        assert!(
            extract("empty.js", b"// empty")
                .unwrap()
                .evidence
                .is_empty()
        );
    }

    #[test]
    fn callers_name_their_enclosing_function_and_requires_are_imports() {
        use crate::syntax::tests::rows;
        let source = b"const fs = require('node:fs');\nconst load = async () => import('./plugin.js');\nclass Store {\n  cache = build();\n  read(p) { return fs.readFileSync(p); }\n  #drop = () => this.#evict();\n}\nexports.run = function () { new Store().read(load()); };\nconst handlers = { start() { boot(); }, stop: () => halt() };\n";
        let result = extract("store.cjs", source).unwrap();
        assert_eq!(
            rows(&result, "declares"),
            [
                ("js:declares-variable", "fs", None),
                ("js:declares-function", "load", None),
                ("js:declares-class", "Store", None),
                ("js:declares-method", "read", None),
                ("js:declares-method", "#drop", None),
                ("js:declares-function", "run", None),
                ("js:declares-variable", "handlers", None),
                ("js:declares-method", "start", None),
                ("js:declares-method", "stop", None),
            ]
        );
        assert_eq!(
            rows(&result, "invokes"),
            [
                ("js:invokes-syntax", "build", None),
                ("js:invokes-syntax", "readFileSync", Some("read")),
                ("js:invokes-syntax", "#evict", Some("#drop")),
                ("js:invokes-syntax", "read", Some("run")),
                ("js:invokes-syntax", "Store", Some("run")),
                ("js:invokes-syntax", "load", Some("run")),
                ("js:invokes-syntax", "boot", Some("start")),
                ("js:invokes-syntax", "halt", Some("stop")),
            ]
        );
        assert_eq!(
            rows(&result, "import"),
            [
                ("js:imports", "node:fs", None),
                ("js:dynamic-import", "./plugin.js", None),
            ]
        );
    }
}
