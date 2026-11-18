//! What one changed file does, as a `review` standard's `when` reads it: facts over the
//! lines the change adds and removes, the imports it adds and where the file sits, never a
//! judgement of what the change means.
//! - `behavior`: a line other than a comment or a blank one, added or removed in a file that
//!   is not a test; in a document every line is content.
//! - `structure`: an import of another declared module, a file in a top-level folder that is
//!   new, or a new file in a language its module does not declare.
//! - `prose`: a comment line or a document line added, or the file is in a commit the session
//!   made, whose message is prose too.
use super::{Import, owner};
use crate::debt::COMMENTS;
use serde_json::Value;

/// The facts a `when` names.
pub const FACTS: &[&str] = &["behavior", "structure", "prose"];

/// The lines a change adds, each with its line in the new file, and the lines it removes.
pub type Lines<'a> = (&'a [(usize, String)], &'a [String]);

/// One changed file, named from the project root, with what the stop read about it.
pub struct Change<'a> {
    pub file: &'a str,
    /// The lines changed against the stop's base; `None` when they could not be compared,
    /// which counts as both `behavior` and `prose`.
    pub lines: Option<Lines<'a>>,
    /// Absent from the base.
    pub created: bool,
    /// In a top-level folder absent from the base.
    pub new_folder: bool,
    /// The imports the change adds, resolved.
    pub imports: &'a [Import],
    /// In a commit the session made.
    pub committed: bool,
}

/// Folder names that hold tests, and file-name forms of a test across ecosystems.
const TEST_FOLDERS: &[&str] = &["test", "tests", "__tests__", "spec", "specs"];
const TEST_NAMES: &[&str] = &["tests.rs", "conftest.py"];
const TEST_INFIXES: &[&str] = &[".test.", ".spec.", "_test.", "_spec."];
const TEST_SUFFIXES: &[&str] = &["Test", "Tests", "Spec", "IT"];
/// Extensions of documents, where a `#` or `*` line is content, not a comment.
const DOCUMENTS: &[&str] = &["md", "mdx", "markdown", "rst", "adoc", "txt"];

/// Whether `file` is a test by its folder or its name.
pub fn test_file(file: &str) -> bool {
    let mut segments: Vec<&str> = file.split('/').collect();
    let name = segments.pop().unwrap_or_default();
    if segments
        .iter()
        .any(|segment| TEST_FOLDERS.contains(segment))
    {
        return true;
    }
    let stem = name.split('.').next().unwrap_or(name);
    TEST_NAMES.contains(&name)
        || name.starts_with("test_")
        || TEST_INFIXES.iter().any(|infix| name.contains(infix))
        || (name.contains('.')
            && TEST_SUFFIXES
                .iter()
                .any(|suffix| stem.len() > suffix.len() && stem.ends_with(suffix)))
}

fn document(file: &str) -> bool {
    let name = file.rsplit('/').next().unwrap_or(file);
    name.rsplit_once('.')
        .is_some_and(|(_, extension)| DOCUMENTS.contains(&extension.to_ascii_lowercase().as_str()))
}

fn comment(line: &str) -> bool {
    let line = line.trim_start();
    COMMENTS.iter().any(|prefix| line.starts_with(prefix))
}

/// Whether `change` adds an import that resolves into another declared module than the
/// file's own, or is a new file in a language its module does not declare.
fn crosses(settings: &Value, change: &Change) -> bool {
    let Some(module) = owner(settings, change.file) else {
        return false;
    };
    let name = &module["name"];
    let imports = change.imports.iter().any(|import| {
        import
            .resolved
            .as_deref()
            .and_then(|path| owner(settings, path))
            .is_some_and(|target| target["name"] != *name)
    });
    let foreign = change.created
        && module["languages"].as_array().is_some_and(|declared| {
            crate::routing::scope(settings, &[change.file.into()]).is_ok_and(|scope| {
                scope
                    .languages
                    .iter()
                    .any(|language| !declared.contains(language))
            })
        });
    imports || foreign
}

/// The facts `change` holds, in [`FACTS`] order.
pub fn facts(settings: &Value, change: &Change) -> Vec<&'static str> {
    let (behavior, prose) = match change.lines {
        None => (true, true),
        Some((added, removed)) => {
            let content =
                |line: &str| !line.trim().is_empty() && (document(change.file) || !comment(line));
            let behavior = !test_file(change.file)
                && added
                    .iter()
                    .map(|(_, line)| line.as_str())
                    .chain(removed.iter().map(String::as_str))
                    .any(content);
            let prose = added.iter().any(|(_, line)| {
                !line.trim().is_empty() && (document(change.file) || comment(line))
            });
            (behavior, prose)
        }
    };
    let structure = change.new_folder || crosses(settings, change);
    FACTS
        .iter()
        .zip([behavior, structure, prose || change.committed])
        .filter_map(|(fact, holds)| holds.then_some(*fact))
        .collect()
}

#[cfg(test)]
mod tests;
