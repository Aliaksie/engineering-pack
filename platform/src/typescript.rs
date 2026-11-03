//! TypeScript and TSX syntax relationships, read with the JavaScript patterns plus TypeScript
//! declarations. No type checking: a call names a token, not a resolved callee.
use crate::syntax::{Language, Vocabulary};

pub const PROVIDER: &str = "typescript-tree-sitter-0.23.2/v1";
pub const TSX_PROVIDER: &str = "tsx-tree-sitter-0.23.2/v1";

const KINDS: &[&str] = &[
    "ts:declares-class",
    "ts:declares-enum",
    "ts:declares-function",
    "ts:declares-interface",
    "ts:declares-method",
    "ts:declares-module",
    "ts:declares-type",
    "ts:declares-variable",
    "ts:dynamic-import",
    "ts:imports",
    "ts:imports-name",
    "ts:invokes-syntax",
];

fn query() -> String {
    [
        include_str!("../queries/ecmascript.scm"),
        include_str!("../queries/typescript.scm"),
    ]
    .join("\n")
}

pub const LANGUAGE: Language = Language {
    name: "ts",
    provider: PROVIDER,
    extensions: &["ts", "mts", "cts"],
    grammar: || tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
    query,
    vocabulary: Vocabulary::Craft,
    kinds: KINDS,
    import: None,
};

pub const TSX: Language = Language {
    name: "ts",
    provider: TSX_PROVIDER,
    extensions: &["tsx"],
    grammar: || tree_sitter_typescript::LANGUAGE_TSX.into(),
    query,
    vocabulary: Vocabulary::Craft,
    kinds: KINDS,
    import: None,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::tests::rows;

    #[test]
    fn declarations_calls_and_imports() {
        let source = b"import { HTTPException } from './http-exception'\nimport type { Env } from './types'\nexport interface Options { realm?: string }\nexport type Handler = () => void\nenum Kind { A }\nexport class Guard {\n  private check = (t: string) => verify(t)\n  run(): void { this.#deny(); throw new HTTPException(401) }\n  #deny() {}\n}\nexport const bearerAuth = (o: Options): Handler => {\n  return async () => { await next() }\n}\n";
        let result = LANGUAGE.extract("src/auth.ts", source).unwrap();
        assert_eq!(
            rows(&result, "declares"),
            [
                ("ts:declares-interface", "Options", None),
                ("ts:declares-type", "Handler", None),
                ("ts:declares-enum", "Kind", None),
                ("ts:declares-class", "Guard", None),
                ("ts:declares-method", "check", None),
                ("ts:declares-method", "run", None),
                ("ts:declares-method", "#deny", None),
                ("ts:declares-function", "bearerAuth", None),
            ]
        );
        assert_eq!(
            rows(&result, "invokes"),
            [
                ("ts:invokes-syntax", "verify", Some("check")),
                ("ts:invokes-syntax", "#deny", Some("run")),
                ("ts:invokes-syntax", "HTTPException", Some("run")),
                ("ts:invokes-syntax", "next", Some("bearerAuth")),
            ]
        );
        assert_eq!(
            rows(&result, "imports"),
            [
                ("ts:imports", "./http-exception", None),
                ("ts:imports-name", "HTTPException", None),
                ("ts:imports", "./types", None),
                ("ts:imports-name", "Env", None),
            ]
        );
        let tsx = TSX
            .extract(
                "src/a.tsx",
                b"export function View() { return <div>{render()}</div> }",
            )
            .unwrap();
        assert_eq!(
            rows(&tsx, "invokes"),
            [("ts:invokes-syntax", "render", Some("View"))]
        );
    }

    /// The grammar rejects some valid TypeScript (here an overload call signature in an
    /// interface, as in hono's `src/context.ts`): what lies outside the error is still cited,
    /// nothing recovered inside it is, and the source is marked partial.
    #[test]
    fn grammar_errors_keep_evidence_outside_them() {
        let source = b"interface NewResponse {\n  (data: string): Response\n  <T>(data: T): T extends string ? Response : Promise<Response>\n}\nexport class Context {\n  json(o: object) { return render(o) }\n}\n";
        let result = LANGUAGE.extract("src/context.ts", source).unwrap();
        assert!(result.partial);
        let declared: Vec<_> = rows(&result, "declares").into_iter().map(|r| r.1).collect();
        assert!(
            declared.contains(&"Context") && declared.contains(&"json"),
            "{declared:?}"
        );
        assert_eq!(
            rows(&result, "invokes"),
            [("ts:invokes-syntax", "render", Some("json"))]
        );
        assert!(
            !LANGUAGE
                .extract("src/ok.ts", b"export const a = 1\n")
                .unwrap()
                .partial
        );
    }
}
