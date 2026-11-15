//! One query-driven syntax extractor for every language. A language is data: a grammar, a
//! query that names declarations, references and imports, and at most a small function for an
//! import spelling a query cannot normalize. Evidence is syntax only: a reference names a token,
//! never a resolved target.
use craft_engine::{Evidence, Extraction, hash, identity, valid_path};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tree_sitter::{
    Node, ParseOptions, Parser, Query, QueryCursor, QueryPredicate, QueryPredicateArg,
    StreamingIterator,
};

pub const MAX_SOURCE_BYTES: usize = 2 * 1024 * 1024;
const MAX_RECORDS: usize = 20_000;

/// An import target from the captured import node, or `None` to record nothing.
pub type Import = fn(Node, &str) -> Option<String>;

/// How a query's capture names map to evidence kinds.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Vocabulary {
    /// Craft's own queries: `@declares[.sub]`, `@invokes`, `@references`, `@imports[.sub]`,
    /// `@dynamic-import`, with `@name`, `@target`, `@container` and `@boundary`.
    Craft,
    /// A grammar's shipped tags query: `@definition.X` declares, `@reference.call` invokes,
    /// and function or method definitions own the calls inside them.
    Tags,
}

pub struct Language {
    /// Evidence kind prefix: `java` gives `java:declares-type`.
    pub name: &'static str,
    pub provider: &'static str,
    pub extensions: &'static [&'static str],
    pub grammar: fn() -> tree_sitter::Language,
    pub query: fn() -> String,
    pub vocabulary: Vocabulary,
    /// Every evidence kind the query can produce, declared so a lookup can choose providers
    /// without compiling queries; a test holds it equal to the query's captures.
    pub kinds: &'static [&'static str],
    pub import: Option<Import>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Role {
    Declares,
    Invokes,
    Imports,
}

/// What one capture name means.
enum Meaning {
    Fact(Role, String),
    Name,
    Target,
    Container,
    Boundary,
    Other,
}

impl Language {
    fn meaning(&self, capture: &str) -> Meaning {
        let kind = |suffix: &str| format!("{}:{suffix}", self.name);
        match (self.vocabulary, capture) {
            (_, "name") => Meaning::Name,
            (Vocabulary::Craft, "target") => Meaning::Target,
            (Vocabulary::Craft, "container") => Meaning::Container,
            (Vocabulary::Craft, "boundary") => Meaning::Boundary,
            (Vocabulary::Craft, "invokes") => Meaning::Fact(Role::Invokes, kind("invokes-syntax")),
            (Vocabulary::Craft, "references") => {
                Meaning::Fact(Role::Invokes, kind("references-syntax"))
            }
            (Vocabulary::Craft, "dynamic-import") => {
                Meaning::Fact(Role::Imports, kind("dynamic-import"))
            }
            (Vocabulary::Craft, capture)
                if capture == "declares" || capture.starts_with("declares.") =>
            {
                Meaning::Fact(Role::Declares, kind(&capture.replace('.', "-")))
            }
            (Vocabulary::Craft, capture)
                if capture == "imports" || capture.starts_with("imports.") =>
            {
                Meaning::Fact(Role::Imports, kind(&capture.replace('.', "-")))
            }
            (Vocabulary::Tags, "reference.call" | "reference.send") => {
                Meaning::Fact(Role::Invokes, kind("invokes-syntax"))
            }
            (Vocabulary::Tags, capture) if capture.starts_with("definition.") => Meaning::Fact(
                Role::Declares,
                kind(&format!("declares-{}", &capture["definition.".len()..])),
            ),
            _ => Meaning::Other,
        }
    }

    /// The compiled query, built once per process. The queries are part of this binary, so a
    /// query that does not compile is a defect, caught by the tests.
    fn compiled(&self) -> &'static Query {
        static QUERIES: OnceLock<std::sync::Mutex<HashMap<&'static str, &'static Query>>> =
            OnceLock::new();
        let mut queries = QUERIES
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        queries.entry(self.provider).or_insert_with(|| {
            let query = Query::new(&(self.grammar)(), &(self.query)())
                .unwrap_or_else(|e| panic!("{} query: {e}", self.name));
            Box::leak(Box::new(query))
        })
    }

    /// The evidence kinds the compiled query's captures name.
    #[cfg(test)]
    pub(crate) fn query_kinds(&self) -> Vec<String> {
        let mut kinds: Vec<String> = self
            .compiled()
            .capture_names()
            .iter()
            .filter_map(|capture| match self.meaning(capture) {
                Meaning::Fact(_, kind) => Some(kind),
                _ => None,
            })
            .collect();
        kinds.sort();
        kinds.dedup();
        kinds
    }

    /// Each kind the query produces with whether its capture names a call or reference token.
    #[cfg(test)]
    pub(crate) fn kind_roles(&self) -> Vec<(String, bool)> {
        self.compiled()
            .capture_names()
            .iter()
            .filter_map(|capture| match self.meaning(capture) {
                Meaning::Fact(role, kind) => Some((kind, role == Role::Invokes)),
                _ => None,
            })
            .collect()
    }

    pub fn extract(&self, path: &str, bytes: &[u8]) -> Result<Extraction, String> {
        if !valid_path(path) || bytes.len() > MAX_SOURCE_BYTES {
            return Err("source exceeds path/byte bounds".into());
        }
        let source = std::str::from_utf8(bytes).map_err(|_| "source is not UTF-8")?;
        // Compiled once per process, before this source's deadline starts.
        let query = self.compiled();
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut parser = Parser::new();
        parser
            .set_language(&(self.grammar)())
            .map_err(|e| e.to_string())?;
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
        // A grammar may reject valid source (TypeScript overload signatures, newer syntax).
        // Facts outside syntax errors are kept and the source is marked partial.
        let partial = tree.root_node().has_error();
        let meanings: Vec<Meaning> = query
            .capture_names()
            .iter()
            .map(|c| self.meaning(c))
            .collect();
        let mut facts: Vec<Fact> = Vec::new();
        let mut seen: HashMap<(Role, usize, usize), usize> = HashMap::new();
        let mut containers = HashSet::new();
        let mut boundaries = HashSet::new();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(query, tree.root_node(), bytes);
        while let Some(found) = matches.next() {
            if Instant::now() >= deadline {
                return Err("extraction deadline exceeded".into());
            }
            if !query
                .general_predicates(found.pattern_index)
                .iter()
                .all(|p| holds(p, found.captures()))
            {
                continue;
            }
            let (mut fact, mut name, mut target, mut container) = (None, None, None, false);
            for capture in found.captures() {
                match &meanings[capture.index as usize] {
                    Meaning::Fact(role, kind) => fact = Some((role.to_owned(), kind, capture.node)),
                    Meaning::Name => name = Some(capture.node),
                    Meaning::Target => target = Some(capture.node),
                    Meaning::Container => container = true,
                    Meaning::Boundary => {
                        boundaries.insert(capture.node.id());
                    }
                    Meaning::Other => {}
                }
            }
            let Some((role, kind, node)) = fact else {
                continue;
            };
            if partial && (damaged(node) || name.is_some_and(|n| n.is_missing())) {
                continue;
            }
            let pattern = found.pattern_index;
            let fact = match role {
                Role::Declares => {
                    let Some(name) = name else { continue };
                    let tags = self.vocabulary == Vocabulary::Tags
                        && (kind.ends_with("-function") || kind.ends_with("-method"));
                    Fact {
                        role,
                        kind: kind.clone(),
                        name: source[name.byte_range()].into(),
                        target: None,
                        node,
                        anchor: node,
                        container: container || tags,
                        pattern,
                    }
                }
                Role::Invokes => {
                    let Some(name) = name else { continue };
                    let name_text = source[name.byte_range()].to_string();
                    Fact {
                        role,
                        kind: kind.clone(),
                        target: Some(name_text.clone()),
                        name: name_text,
                        node,
                        anchor: name,
                        container: false,
                        pattern,
                    }
                }
                Role::Imports => {
                    let target = match self.import {
                        Some(import) => match import(node, source) {
                            Some(target) => Some(target),
                            None => continue,
                        },
                        None => target.and_then(|t| unquote(&source[t.byte_range()])),
                    };
                    // A named import is cited by the name it imports, from its module.
                    let imported = name.map(|n| source[n.byte_range()].to_string());
                    Fact {
                        role,
                        kind: kind.clone(),
                        name: imported
                            .or_else(|| target.clone())
                            .unwrap_or_else(|| "unresolved import".into()),
                        target,
                        node,
                        anchor: node,
                        container: false,
                        pattern,
                    }
                }
            };
            // Several patterns may describe one node (a function bound to a variable, an import
            // with and without a literal target): keep one fact, with the kind of the earliest
            // pattern and what any of them knew.
            let key = (fact.role, fact.anchor.start_byte(), fact.anchor.end_byte());
            match seen.get(&key) {
                Some(&index) => {
                    let kept = &mut facts[index];
                    kept.container |= fact.container;
                    if fact.pattern < kept.pattern {
                        kept.pattern = fact.pattern;
                        kept.kind = fact.kind;
                    }
                    if kept.target.is_none() && fact.target.is_some() {
                        kept.name = fact.name;
                        kept.target = fact.target;
                    }
                }
                None => {
                    seen.insert(key, facts.len());
                    facts.push(fact);
                }
            }
            if facts.len() > MAX_RECORDS {
                return Err("extraction record bound exceeded".into());
            }
        }
        // Source order, an enclosing node before what it encloses.
        facts.sort_by_key(|f| {
            (
                f.node.start_byte(),
                std::cmp::Reverse(f.node.end_byte()),
                f.anchor.start_byte(),
            )
        });
        let id = |f: &Fact| {
            identity(&[
                path,
                self.provider,
                &f.kind,
                &f.name,
                &f.anchor.start_byte().to_string(),
            ])
        };
        for fact in facts.iter().filter(|f| f.container) {
            containers.insert(fact.node.id());
        }
        let owners: HashMap<usize, String> = facts
            .iter()
            .filter(|f| f.container)
            .map(|f| (f.node.id(), id(f)))
            .collect();
        let evidence = facts
            .iter()
            .map(|f| Evidence {
                id: id(f),
                kind: f.kind.clone(),
                name: f.name.clone(),
                line: f.anchor.start_position().row + 1,
                column: f.anchor.start_position().column + 1,
                start_byte: f.anchor.start_byte(),
                end_byte: f.anchor.end_byte(),
                target: f.target.clone(),
                container: (f.role == Role::Invokes)
                    .then(|| owner(f.node, &containers, &boundaries))
                    .flatten()
                    .and_then(|node| owners.get(&node).cloned()),
            })
            .collect();
        let result = Extraction {
            path: path.into(),
            hash: hash(bytes),
            provider: self.provider.into(),
            evidence,
            partial,
        };
        result.validate()?;
        Ok(result)
    }
}

struct Fact<'tree> {
    role: Role,
    kind: String,
    name: String,
    target: Option<String>,
    /// The captured node: a declaration, a reference expression or an import.
    node: Node<'tree>,
    /// The cited span: a declaration or import node, or a reference's name token.
    anchor: Node<'tree>,
    container: bool,
    /// The query pattern that named the kind.
    pattern: usize,
}

/// A node that is a syntax error or lies inside one: what the parser recovered there is not
/// evidence. A declaration whose body holds an error is still declared.
fn damaged(node: Node) -> bool {
    if node.is_error() || node.is_missing() {
        return true;
    }
    let mut parent = node.parent();
    while let Some(ancestor) = parent {
        if ancestor.is_error() {
            return true;
        }
        parent = ancestor.parent();
    }
    false
}

/// The nearest enclosing container of a reference, unless a boundary comes first: a nested
/// type or impl body does not run as the function around it.
fn owner(node: Node, containers: &HashSet<usize>, boundaries: &HashSet<usize>) -> Option<usize> {
    let mut parent = node.parent();
    while let Some(node) = parent {
        if boundaries.contains(&node.id()) {
            return None;
        }
        if containers.contains(&node.id()) {
            return Some(node.id());
        }
        parent = node.parent();
    }
    None
}

/// A quoted literal's text (or a C `<system>` include); an escape needs the language's
/// interpretation and stays unresolved. Unquoted text (a dotted module name) is returned as is.
fn unquote(raw: &str) -> Option<String> {
    let quoted = raw.len() >= 2
        && [('"', '"'), ('\'', '\''), ('`', '`'), ('<', '>')]
            .iter()
            .any(|(open, close)| raw.starts_with(*open) && raw.ends_with(*close));
    match quoted {
        true if raw.contains('\\') => None,
        true => Some(raw[1..raw.len() - 1].into()),
        false => Some(raw.into()),
    }
}

/// Structural predicates a query cannot state with tree-sitter's text predicates:
/// - `(#nearest? @c kind other...)`: of the listed ancestor kinds, the nearest is `kind`;
/// - `(#adjacent? @a @b)`: `@b` is the node right after `@a`, anonymous tokens included;
/// - `(#not-after? @c kind)`: the node before `@c`, anonymous included, is not `kind`;
/// - `(#has? @c kind)` / `(#not-has? @c kind)`: `@c` has (no) child of that kind.
fn holds(predicate: &QueryPredicate, captures: &[tree_sitter::QueryCapture]) -> bool {
    let node = |arg: &QueryPredicateArg| match arg {
        QueryPredicateArg::Capture(index) => {
            captures.iter().find(|c| c.index == *index).map(|c| c.node)
        }
        QueryPredicateArg::String(_) => None,
    };
    let strings: Vec<&str> = predicate
        .args
        .iter()
        .filter_map(|arg| match arg {
            QueryPredicateArg::String(s) => Some(&**s),
            QueryPredicateArg::Capture(_) => None,
        })
        .collect();
    // Directives of a grammar's tags query (`#strip!`, `#select-adjacent!`) shape
    // documentation captures this extractor does not use.
    if predicate.operator.ends_with('!') {
        return true;
    }
    let Some(subject) = predicate.args.first().and_then(node) else {
        return false;
    };
    let has = || {
        let mut cursor = subject.walk();
        subject
            .children(&mut cursor)
            .any(|child| strings.contains(&child.kind()))
    };
    match &*predicate.operator {
        "nearest?" => {
            let mut parent = subject.parent();
            while let Some(ancestor) = parent {
                if let Some(position) = strings.iter().position(|k| *k == ancestor.kind()) {
                    return position == 0;
                }
                parent = ancestor.parent();
            }
            false
        }
        "adjacent?" => predicate
            .args
            .get(1)
            .and_then(node)
            .is_some_and(|next| subject.next_sibling().is_some_and(|n| n.id() == next.id())),
        "not-after?" => subject
            .prev_sibling()
            .is_none_or(|before| !strings.contains(&before.kind())),
        "has?" => has(),
        "not-has?" => !has(),
        other => panic!("unknown query predicate {other}"),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// `(kind, name, owner name)` of the evidence whose kind contains `role`, in order.
    pub(crate) fn rows<'a>(
        result: &'a Extraction,
        role: &str,
    ) -> Vec<(&'a str, &'a str, Option<&'a str>)> {
        result
            .evidence
            .iter()
            .filter(|e| e.kind.contains(role))
            .map(|e| {
                let owner = result
                    .evidence
                    .iter()
                    .find(|o| Some(&o.id) == e.container.as_ref());
                (
                    e.kind.as_str(),
                    e.name.as_str(),
                    owner.map(|o| o.name.as_str()),
                )
            })
            .collect()
    }

    #[test]
    fn quoted_targets_are_unquoted_and_escapes_stay_unresolved() {
        assert_eq!(unquote("'./a.js'").as_deref(), Some("./a.js"));
        assert_eq!(unquote("\"fmt\"").as_deref(), Some("fmt"));
        assert_eq!(unquote("os.path").as_deref(), Some("os.path"));
        assert_eq!(unquote("<stdio.h>").as_deref(), Some("stdio.h"));
        assert_eq!(unquote("'a\\u0062'"), None);
    }
}
