//! Authored text the model reads, kept in `text/` files of the crate that renders it and
//! embedded in the executable: nothing is read from disk at run time.
//!
//! A template names its slots. `{name}` in the text is replaced by the value given for `name`
//! in one pass, so an inserted value is never read as template syntax; `{{` and `}}` write a
//! literal brace. Every template's identity is its `id` with the SHA-256 of its exact bytes,
//! so recorded evaluation evidence can name the wording a run was given.

use serde_json::{Map, Value, json};

/// One embedded template.
pub struct Template {
    /// Stable identity: unchanged by wording edits.
    pub id: &'static str,
    /// The slot names the text uses, each at least once.
    pub slots: &'static [&'static str],
    /// The file's bytes; one final newline is not part of the text.
    pub source: &'static str,
}

/// Template text between its slots: literal text, or the slot with this name.
enum Part<'a> {
    Text(&'a str),
    Slot(&'a str),
}

impl Template {
    /// The text without the file's final newline.
    pub fn text(&self) -> &'static str {
        self.source.strip_suffix('\n').unwrap_or(self.source)
    }

    /// SHA-256 of the file's bytes, lowercase hex.
    pub fn hash(&self) -> String {
        crate::hash(self.source.as_bytes())
    }

    /// The text with each slot replaced by its value. A slot without a value renders empty;
    /// [`check`] keeps every template's slots and its declared names equal.
    pub fn render(&self, values: &[(&str, &str)]) -> String {
        let mut out = String::with_capacity(self.source.len());
        for part in parts(self.text()).into_iter().flatten() {
            match part {
                Part::Text(text) => out.push_str(text),
                Part::Slot(name) => out.push_str(
                    values
                        .iter()
                        .find(|(slot, _)| *slot == name)
                        .map_or("", |(_, value)| value),
                ),
            }
        }
        out
    }
}

/// `text` split into literal text and slots; `None` when a brace is unmatched or a slot is
/// not a lowercase name.
fn parts(text: &str) -> Option<Vec<Part<'_>>> {
    let mut parts = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find(['{', '}']) {
        parts.push(Part::Text(&rest[..at]));
        let brace = &rest[at..at + 1];
        if rest[at + 1..].starts_with(brace) {
            parts.push(Part::Text(brace));
            rest = &rest[at + 2..];
            continue;
        }
        if brace == "}" {
            return None;
        }
        let close = rest[at..].find('}')? + at;
        let name = &rest[at + 1..close];
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_lowercase() || b == b'-') {
            return None;
        }
        parts.push(Part::Slot(name));
        rest = &rest[close + 1..];
    }
    parts.push(Part::Text(rest));
    Some(parts)
}

/// Problems of a template set: duplicate ids, malformed braces, and slots used but not
/// declared or declared but not used.
pub fn check(templates: &[&Template]) -> Vec<String> {
    let mut problems = Vec::new();
    for (index, template) in templates.iter().enumerate() {
        if templates[..index].iter().any(|t| t.id == template.id) {
            problems.push(format!("{}: duplicate id", template.id));
        }
        let Some(parts) = parts(template.text()) else {
            problems.push(format!("{}: unmatched brace or invalid slot", template.id));
            continue;
        };
        let used: Vec<&str> = parts
            .iter()
            .filter_map(|part| match part {
                Part::Slot(name) => Some(*name),
                Part::Text(_) => None,
            })
            .collect();
        for name in &used {
            if !template.slots.contains(name) {
                problems.push(format!("{}: undeclared slot {{{name}}}", template.id));
            }
        }
        for name in template.slots {
            if !used.contains(name) {
                problems.push(format!("{}: declared slot {name} is unused", template.id));
            }
        }
    }
    problems
}

/// `{id: sha256}` for a template set, in the order given.
pub fn identities(templates: &[&Template]) -> Value {
    Value::Object(Map::from_iter(
        templates
            .iter()
            .map(|template| (template.id.to_owned(), json!(template.hash()))),
    ))
}

/// The engine's own model-facing templates.
pub fn templates() -> Vec<&'static Template> {
    crate::safety::TEMPLATES.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn template(source: &'static str, slots: &'static [&'static str]) -> Template {
        Template {
            id: "test",
            slots,
            source,
        }
    }

    #[test]
    fn values_are_inserted_once_and_never_read_as_slots() {
        let t = template("A {one} and {two}, {{literal}}.\n", &["one", "two"]);
        assert_eq!(
            t.render(&[("one", "{two}"), ("two", "ünïcode \"quoted\"\nline")]),
            "A {two} and ünïcode \"quoted\"\nline, {literal}."
        );
        assert_eq!(t.render(&[("one", ""), ("two", "")]), "A  and , {literal}.");
        assert!(check(&[&t]).is_empty());
    }

    #[test]
    fn only_one_final_newline_is_dropped() {
        assert_eq!(template("text\n\n", &[]).text(), "text\n");
        assert_eq!(template("text", &[]).text(), "text");
    }

    #[test]
    fn check_reports_slots_braces_and_duplicates() {
        let undeclared = template("{a} {b}", &["a"]);
        let unused = template("{a}", &["a", "b"]);
        let open = template("an { open", &[]);
        let close = template("a } close", &[]);
        let spaced = template("{not a slot}", &[]);
        let problems = check(&[&undeclared, &unused, &open, &close, &spaced]);
        assert!(problems.contains(&"test: undeclared slot {b}".to_owned()));
        assert!(problems.contains(&"test: declared slot b is unused".to_owned()));
        assert!(problems.contains(&"test: duplicate id".to_owned()));
        assert_eq!(
            problems
                .iter()
                .filter(|p| p.ends_with("unmatched brace or invalid slot"))
                .count(),
            3
        );
    }

    #[test]
    fn the_hash_follows_the_exact_bytes() {
        let a = template("same words\n", &[]);
        let b = template("same words \n", &[]);
        assert_ne!(a.hash(), b.hash());
        assert_eq!(a.hash(), crate::hash(b"same words\n"));
        assert_eq!(identities(&[&a])["test"], json!(a.hash()));
    }

    #[test]
    fn engine_templates_are_well_formed() {
        assert_eq!(check(&templates()), Vec::<String>::new());
    }
}
