//! Syntax evidence only: no claims about runtime calls or package resolution.
use craft_engine::{Evidence, Extraction, hash, identity, valid_path};
use std::time::{Duration, Instant};
use tree_sitter::{ParseOptions, Parser};

pub const PROVIDER: &str = "javascript-tree-sitter-0.25.0/v1";
pub const MAX_SOURCE_BYTES: usize = 2 * 1024 * 1024;

pub fn extract(path: &str, bytes: &[u8]) -> Result<Extraction, String> {
    if !valid_path(path) || bytes.len() > MAX_SOURCE_BYTES {
        return Err("source exceeds path/byte bounds".into());
    }
    let source = std::str::from_utf8(bytes).map_err(|_| "source is not UTF-8")?;
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut cancel = |_: &tree_sitter::ParseState| {
        if Instant::now() >= deadline {
            std::ops::ControlFlow::Break(())
        } else {
            std::ops::ControlFlow::Continue(())
        }
    };
    let tree = parser
        .parse_with_options(
            &mut |offset, _| &bytes[offset..],
            None,
            Some(ParseOptions::new().progress_callback(&mut cancel)),
        )
        .ok_or("parser deadline exceeded")?;
    if tree.root_node().has_error() {
        return Err("JavaScript syntax error".into());
    }
    let mut evidence = Vec::new();
    let mut cursor = tree.walk();
    loop {
        if Instant::now() >= deadline {
            return Err("extraction deadline exceeded".into());
        }
        let node = cursor.node();
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
                (!raw.contains('\\') && raw.len() >= 2).then(|| raw[1..raw.len() - 1].to_string())
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
            evidence.push(Evidence {
                id: identity(&[
                    path,
                    PROVIDER,
                    predicate,
                    &name,
                    &node.start_byte().to_string(),
                ]),
                kind: predicate.into(),
                name,
                line: node.start_position().row + 1,
                column: node.start_position().column + 1,
                start_byte: node.start_byte(),
                end_byte: node.end_byte(),
                target,
            });
            if evidence.len() > 20_000 {
                return Err("extraction record bound exceeded".into());
            }
        }
        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                let result = Extraction {
                    path: path.into(),
                    hash: hash(bytes),
                    provider: PROVIDER.into(),
                    evidence,
                };
                result.validate()?;
                return Ok(result);
            }
        }
    }
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
