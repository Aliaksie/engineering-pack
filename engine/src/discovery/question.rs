//! Structural lookups: Project Intelligence answers what it covers in the requested scope and
//! a literal search covers the rest, or the whole scope when the index cannot answer.
use super::scope;
use crate::values::js_length;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    Declaration,
    Callers,
    Imports,
    Section,
    Outline,
}

pub const ASKS: &[(&str, Ask)] = &[
    ("declaration", Ask::Declaration),
    ("callers", Ask::Callers),
    ("imports", Ask::Imports),
    ("section", Ask::Section),
    ("outline", Ask::Outline),
];

/// Source code that could declare, call or import a name, whether or not a provider reads it.
const CODE: &[&str] = &[
    "c", "cc", "cjs", "clj", "cpp", "cs", "cts", "cxx", "dart", "ex", "exs", "go", "groovy", "h",
    "hpp", "java", "js", "jsx", "kt", "kts", "lua", "m", "mjs", "mm", "mts", "php", "py", "rb",
    "rs", "scala", "swift", "ts", "tsx",
];
/// Documents with headings.
const DOCUMENTS: &[&str] = &["adoc", "asciidoc", "markdown", "md", "org", "rst"];

impl Ask {
    pub fn name(self) -> &'static str {
        ASKS.iter().find(|(_, ask)| *ask == self).unwrap().0
    }

    /// The evidence field matched and the kinds that answer this question.
    pub fn evidence(self) -> (&'static str, &'static [&'static str]) {
        match self {
            Ask::Declaration => (
                "name",
                &[
                    "java:declares-constructor",
                    "java:declares-method",
                    "java:declares-type",
                    "js:declares",
                ],
            ),
            Ask::Callers => ("name", &["java:invokes-syntax"]),
            Ask::Imports => (
                "target",
                &[
                    "java:imports",
                    "java:imports-static",
                    "js:dynamic-import",
                    "js:imports",
                ],
            ),
            Ask::Section => ("name", &["md:section"]),
            Ask::Outline => ("path", &["md:section"]),
        }
    }

    /// Whether a source with this extension could hold an answer. A candidate that no
    /// provider answers for leaves the scope's coverage incomplete.
    pub fn candidate(self, path: &str) -> bool {
        let extension = path
            .rsplit_once('/')
            .map_or(path, |(_, name)| name)
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_ascii_lowercase());
        let Some(extension) = extension else {
            return false;
        };
        match self {
            Ask::Section | Ask::Outline => DOCUMENTS.contains(&extension.as_str()),
            _ => CODE.contains(&extension.as_str()),
        }
    }
}

pub struct Question {
    pub ask: Ask,
    pub value: String,
    /// Normalized relative scopes; `.` is the whole root.
    pub scopes: Vec<String>,
    /// Rows per page, and the evidence ID a continued page starts after.
    pub limit: usize,
    pub after: String,
}

/// One question from `{"ask": name, "value": text, "paths": [scope...], "limit": 1..=200,
/// "after": id}`. An outline names one document, which is its only scope.
pub fn question(input: &Value) -> Result<Question, String> {
    let ask = input["ask"]
        .as_str()
        .and_then(|name| ASKS.iter().find(|(n, _)| *n == name))
        .map(|(_, ask)| *ask)
        .ok_or("question must be declaration, callers, imports, section or outline")?;
    let value = input["value"]
        .as_str()
        .filter(|v| !v.is_empty() && js_length(v) <= 256 && !v.contains(['\r', '\n', '\0']))
        .ok_or("question value must be one nonempty line of at most 256 characters")?
        .to_string();
    let limit = match input.get("limit") {
        None => 30,
        Some(limit) => crate::values::safe_integer(limit)
            .filter(|n| (1..=200).contains(n))
            .ok_or("limit must be an integer from 1 to 200")? as usize,
    };
    let after = match input.get("after") {
        None => String::new(),
        Some(after) => after
            .as_str()
            .filter(|id| crate::values::lower_hex(id, 64))
            .ok_or("after must be an evidence id")?
            .to_string(),
    };
    let paths = input["paths"].as_array().map_or(&[][..], Vec::as_slice);
    if paths.len() > 32 {
        return Err("paths must contain at most 32 relative scopes".into());
    }
    let mut scopes = Vec::new();
    for path in paths {
        let path = scope(path)?;
        if !scopes.contains(&path) {
            scopes.push(path);
        }
    }
    if ask == Ask::Outline {
        let file = scope(&json!(value))?;
        if file == "." || !scopes.is_empty() {
            return Err("an outline names one document and takes no paths".into());
        }
        return Ok(Question {
            ask,
            value: file.clone(),
            scopes: vec![file],
            limit,
            after,
        });
    }
    if scopes.is_empty() || scopes.iter().any(|s| s == ".") {
        scopes = vec![".".into()];
    }
    Ok(Question {
        ask,
        value,
        scopes,
        limit,
        after,
    })
}

/// Whether `path` lies in one of the scopes, by whole path components.
pub fn within(path: &str, scopes: &[String]) -> bool {
    scopes.iter().any(|scope| {
        scope == "."
            || path
                .strip_prefix(scope.as_str())
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    })
}

/// Gap files one combined answer may search literally; more falls back to searching the scope.
pub const MAX_GAP_FILES: usize = 200;

/// Coverage observed for one question's scope just before answering it.
#[derive(Default)]
pub struct Coverage {
    /// `.craft/project.json` enables Project Intelligence.
    pub enabled: bool,
    /// Why the store could not be refreshed or read, if it could not.
    pub unavailable: Option<String>,
    pub inventory_complete: bool,
    /// Scopes lying entirely under a consumer exclusion.
    pub excluded: Vec<String>,
    /// In-scope sources whose provider answers the question but which are not extracted.
    pub pending: Vec<String>,
    pub failed: Vec<String>,
    /// In-scope candidate sources no provider answers for.
    pub unindexed: Vec<String>,
    /// Cited sources that changed after the refresh; their rows are left out.
    pub changed: Vec<String>,
}

impl Coverage {
    /// Every in-scope source the index cannot answer for, in path order.
    pub fn gaps(&self) -> Vec<String> {
        let mut gaps: Vec<_> = [&self.pending, &self.failed, &self.unindexed, &self.changed]
            .into_iter()
            .flatten()
            .cloned()
            .collect();
        gaps.sort();
        gaps.dedup();
        gaps
    }
}

/// How one question is answered. Project Intelligence answers whenever it is enabled, readable
/// and fully inventoried for an unexcluded scope; sources it cannot answer for (`gaps`) are
/// searched literally beside it, up to `MAX_GAP_FILES`. Otherwise, or beyond that, literal
/// search answers the whole scope and `reason` names the first unmet condition.
pub fn route(question: &Question, coverage: &Coverage) -> Value {
    const SHOWN: usize = 8;
    let listed =
        |paths: &[String]| json!({"count": paths.len(), "paths": &paths[..paths.len().min(SHOWN)]});
    let gaps = coverage.gaps();
    let mut detail = Map::new();
    for (name, paths) in [
        ("pending", &coverage.pending),
        ("failed", &coverage.failed),
        ("changed", &coverage.changed),
    ] {
        if !paths.is_empty() {
            detail.insert(name.into(), listed(paths));
        }
    }
    if !coverage.unindexed.is_empty() {
        let mut extensions = BTreeMap::<String, u64>::new();
        for path in &coverage.unindexed {
            let extension = path.rsplit_once('.').map_or("", |(_, e)| e);
            *extensions
                .entry(extension.to_ascii_lowercase())
                .or_default() += 1;
        }
        detail.insert("unindexed".into(), json!(extensions));
    }
    let (answered_by, reason, detail) = if !coverage.enabled {
        ("locate", "index-disabled", Value::Null)
    } else if let Some(why) = &coverage.unavailable {
        ("locate", "index-unavailable", json!(why))
    } else if !coverage.excluded.is_empty() {
        ("locate", "scope-excluded", json!(coverage.excluded))
    } else if !coverage.inventory_complete {
        ("locate", "inventory-incomplete", Value::Null)
    } else if gaps.len() > MAX_GAP_FILES {
        ("locate", "scope-gaps", Value::Object(detail))
    } else if gaps.is_empty() {
        ("intelligence", "covered", Value::Null)
    } else {
        (
            "intelligence+locate",
            "gaps-searched",
            Value::Object(detail),
        )
    };
    let mut route = Map::from_iter([
        ("answeredBy".into(), json!(answered_by)),
        ("question".into(), json!(question.ask.name())),
        ("value".into(), json!(question.value)),
        ("scopes".into(), json!(question.scopes)),
        ("reason".into(), json!(reason)),
    ]);
    if !detail.is_null() {
        route.insert("detail".into(), detail);
    }
    Value::Object(route)
}

/// The literal search for a question over `paths` (the gap files of a combined answer) or,
/// without them, over its whole scope: the spelled name, target or heading, or for an outline
/// the heading marker in that document.
pub fn literal(question: &Question, paths: Option<&[String]>) -> Value {
    let term = match question.ask {
        Ask::Outline => "#",
        _ => question.value.as_str(),
    };
    match paths {
        Some(paths) => json!({"terms": [term], "paths": paths, "maxFiles": paths.len().max(1)}),
        None => json!({"terms": [term], "paths": question.scopes}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(input: Value) -> Question {
        question(&input).unwrap()
    }

    #[test]
    fn questions_normalize_scopes_and_outlines_name_one_document() {
        let q = ask(json!({"ask": "callers", "value": "run", "paths": ["a/", "./a", "b"]}));
        assert_eq!(
            (q.ask, q.scopes),
            (Ask::Callers, vec!["a".into(), "b".into()])
        );
        assert_eq!(
            ask(json!({"ask": "declaration", "value": "X", "paths": ["a", "."]})).scopes,
            ["."]
        );
        let outline = ask(json!({"ask": "outline", "value": "./docs/guide.md"}));
        assert_eq!(
            (outline.value.as_str(), outline.scopes),
            ("docs/guide.md", vec!["docs/guide.md".into()])
        );
        for invalid in [
            json!({"ask": "grep", "value": "x"}),
            json!({"ask": "callers", "value": ""}),
            json!({"ask": "callers", "value": "a\nb"}),
            json!({"ask": "callers", "value": "x", "paths": ["../a"]}),
            json!({"ask": "outline", "value": "."}),
            json!({"ask": "outline", "value": "a.md", "paths": ["a"]}),
            json!({"ask": "callers", "value": "x", "limit": 0}),
            json!({"ask": "callers", "value": "x", "limit": 2.5}),
            json!({"ask": "callers", "value": "x", "after": "ABC"}),
        ] {
            assert!(question(&invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn candidates_follow_the_question_family() {
        assert!(Ask::Callers.candidate("src/lib.RS"));
        assert!(Ask::Declaration.candidate("a/b.tsx"));
        assert!(!Ask::Declaration.candidate("README.md"));
        assert!(Ask::Section.candidate("docs/a.rst"));
        assert!(!Ask::Section.candidate("Makefile"));
        assert!(!Ask::Imports.candidate("dir.d/notes"));
    }

    #[test]
    fn hard_conditions_fall_back_and_gaps_are_searched_beside_the_index() {
        let q = ask(json!({"ask": "callers", "value": "run", "paths": ["src"]}));
        let covered = Coverage {
            enabled: true,
            inventory_complete: true,
            ..Coverage::default()
        };
        let decided = |coverage: &Coverage| {
            let route = route(&q, coverage);
            (route["answeredBy"].clone(), route["reason"].clone())
        };
        assert_eq!(decided(&covered), (json!("intelligence"), json!("covered")));
        let paths = |list: &[&str]| list.iter().map(|p| p.to_string()).collect::<Vec<_>>();
        // Each case also fails every later condition, so order decides the reason.
        let mut coverage = Coverage {
            enabled: true,
            unavailable: Some("project evidence is busy".into()),
            inventory_complete: false,
            excluded: paths(&["src"]),
            pending: paths(&["src/B.java"]),
            failed: paths(&["src/A.java"]),
            unindexed: paths(&["src/lib.rs", "src/x.RS", "src/a.py"]),
            changed: paths(&["src/A.java"]),
        };
        assert_eq!(
            decided(&Coverage {
                enabled: false,
                ..Coverage::default()
            }),
            (json!("locate"), json!("index-disabled"))
        );
        for reason in [
            "index-unavailable",
            "scope-excluded",
            "inventory-incomplete",
        ] {
            assert_eq!(decided(&coverage), (json!("locate"), json!(reason)));
            match reason {
                "index-unavailable" => coverage.unavailable = None,
                "scope-excluded" => coverage.excluded.clear(),
                _ => coverage.inventory_complete = true,
            }
        }
        let combined = route(&q, &coverage);
        assert_eq!(
            (&combined["answeredBy"], &combined["reason"]),
            (&json!("intelligence+locate"), &json!("gaps-searched"))
        );
        assert_eq!(
            combined["detail"],
            json!({"pending": {"count": 1, "paths": ["src/B.java"]}, "failed": {"count": 1, "paths": ["src/A.java"]},
                "changed": {"count": 1, "paths": ["src/A.java"]}, "unindexed": {"py": 1, "rs": 2}})
        );
        assert_eq!(
            coverage.gaps(),
            [
                "src/A.java",
                "src/B.java",
                "src/a.py",
                "src/lib.rs",
                "src/x.RS"
            ]
        );
        let many = (0..=MAX_GAP_FILES).map(|i| format!("src/{i}.rs")).collect();
        let too_many = route(
            &q,
            &Coverage {
                unindexed: many,
                ..covered
            },
        );
        assert_eq!(
            (&too_many["answeredBy"], &too_many["reason"]),
            (&json!("locate"), &json!("scope-gaps"))
        );
        assert_eq!(too_many["detail"]["unindexed"]["rs"], MAX_GAP_FILES + 1);
    }

    #[test]
    fn literal_searches_cover_the_scope_or_only_the_gap_files() {
        let q = ask(json!({"ask": "imports", "value": "./state.js", "paths": ["src"]}));
        assert_eq!(
            literal(&q, None),
            json!({"terms": ["./state.js"], "paths": ["src"]})
        );
        let gaps = ["src/a.ts".to_string(), "src/b.ts".to_string()];
        assert_eq!(
            literal(&q, Some(&gaps)),
            json!({"terms": ["./state.js"], "paths": gaps, "maxFiles": 2})
        );
        let outline = ask(json!({"ask": "outline", "value": "docs/a.md"}));
        assert_eq!(
            literal(&outline, None),
            json!({"terms": ["#"], "paths": ["docs/a.md"]})
        );
        assert!(within("src/a/b.java", &["src/a".into()]));
        assert!(!within("src/ab.java", &["src/a".into()]));
        assert!(within("x", &[".".into()]));
    }
}
