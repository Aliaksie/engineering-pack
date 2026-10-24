//! Shared bounded syntax traversal. Providers assign meanings to syntax nodes.
use craft_engine::{Evidence, Extraction, hash, identity, valid_path};
use std::time::{Duration, Instant};
use tree_sitter::{Language, Node, ParseOptions, Parser, Point};

pub const MAX_SOURCE_BYTES: usize = 2 * 1024 * 1024;

pub struct Fact {
    pub kind: &'static str,
    pub name: String,
    pub target: Option<String>,
    pub container: Option<String>,
    pub anchor: Option<Anchor>,
}

pub struct Anchor {
    pub range: std::ops::Range<usize>,
    pub position: Point,
}

pub fn extract(
    path: &str,
    bytes: &[u8],
    provider: &str,
    language: Language,
    describe: impl Fn(Node, &str) -> Option<Fact>,
) -> Result<Extraction, String> {
    if !valid_path(path) || bytes.len() > MAX_SOURCE_BYTES {
        return Err("source exceeds path/byte bounds".into());
    }
    let source = std::str::from_utf8(bytes).map_err(|_| "source is not UTF-8")?;
    let mut parser = Parser::new();
    parser.set_language(&language).map_err(|e| e.to_string())?;
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
        return Err("source syntax error".into());
    }
    let mut evidence = Vec::new();
    let mut cursor = tree.walk();
    loop {
        if Instant::now() >= deadline {
            return Err("extraction deadline exceeded".into());
        }
        let node = cursor.node();
        if let Some(fact) = describe(node, source) {
            let anchor = fact.anchor.unwrap_or(Anchor {
                range: node.byte_range(),
                position: node.start_position(),
            });
            evidence.push(Evidence {
                id: identity(&[
                    path,
                    provider,
                    fact.kind,
                    &fact.name,
                    &anchor.range.start.to_string(),
                ]),
                kind: fact.kind.into(),
                name: fact.name,
                line: anchor.position.row + 1,
                column: anchor.position.column + 1,
                start_byte: anchor.range.start,
                end_byte: anchor.range.end,
                target: fact.target,
                container: fact.container,
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
                    provider: provider.into(),
                    evidence,
                };
                result.validate()?;
                return Ok(result);
            }
        }
    }
}
