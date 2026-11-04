//! Markdown sections as cited passages: a heading's range runs to the next heading of the
//! same or a higher level, and its container is the enclosing section. A line scanner, not a
//! full CommonMark parser: inside list items and block quotes only an ATX heading opening the
//! item or quote line is recognized, HTML blocks other than comments are not, and inline
//! markup stays as spelled in the name.
//!
//! Outside code, each line also yields what a document points at: local link destinations
//! (inline `[text](path)` and reference definitions), resolved against the document to a
//! root-relative path, and single-token code spans as spelled. Both are cited inside their
//! section. Spans and links that wrap across lines, HTML links and autolinks are not read.
//! A document under an ADR directory (or named `adr-*`) is also a decision record: its first
//! heading is the title and its `Status:` line, front matter `status` or `## Status`
//! section the status, with links in it resolved like any other.
use crate::syntax::MAX_SOURCE_BYTES;
use craft_engine::{Evidence, Extraction, hash, identity, valid_path};

pub const PROVIDER: &str = "markdown-sections/v1";
const KIND: &str = "md:section";
const LINK: &str = "md:link";
const MENTION: &str = "md:mention";
const DECISION: &str = "md:decision";
/// The evidence kinds this provider extracts.
pub const KINDS: &[&str] = &[KIND, LINK, MENTION, DECISION];
/// Directories whose documents are decision records.
const DECISION_DIRECTORIES: &[&str] = &[
    "adr",
    "adrs",
    "decisions",
    "decision-records",
    "architecture-decisions",
];
const STATUS_CHARS: usize = 256;

enum Paragraph {
    None,
    /// A paragraph a setext underline can turn into a heading: first text byte, line, column.
    Plain(usize, usize, usize, Vec<String>),
    /// Lazy continuation of a list item or block quote.
    Nested,
}

/// Columns of leading indentation (tabs to the next multiple of four) and its byte length.
fn indentation(line: &str) -> (usize, usize) {
    let mut columns = 0;
    for (index, byte) in line.bytes().enumerate() {
        match byte {
            b' ' => columns += 1,
            b'\t' => columns += 4 - columns % 4,
            _ => return (columns, index),
        }
    }
    (columns, line.len())
}

fn run(text: &str, marker: u8) -> usize {
    text.bytes().take_while(|b| *b == marker).count()
}

fn fence(text: &str) -> Option<(u8, usize)> {
    let marker = *text.as_bytes().first()?;
    let count = run(text, marker);
    let opens = matches!(marker, b'`' | b'~') && count >= 3;
    // A backtick fence's info string may not contain a backtick.
    (opens && !(marker == b'`' && text[count..].contains('`'))).then_some((marker, count))
}

fn atx(text: &str) -> Option<(usize, String)> {
    let level = run(text, b'#');
    let rest = &text[level..];
    if !(1..=6).contains(&level) || !(rest.is_empty() || rest.starts_with([' ', '\t'])) {
        return None;
    }
    let mut name = rest.trim();
    let closing = name.trim_end_matches('#');
    if closing.is_empty() {
        name = closing;
    } else if closing.len() < name.len() && closing.ends_with([' ', '\t']) {
        name = closing.trim_end();
    }
    Some((level, name.to_string()))
}

fn underline(text: &str) -> Option<usize> {
    let text = text.trim_end();
    let marker = *text.as_bytes().first()?;
    let level = match marker {
        b'=' => 1,
        b'-' => 2,
        _ => return None,
    };
    (run(text, marker) == text.len()).then_some(level)
}

fn thematic_break(text: &str) -> bool {
    let marks: Vec<u8> = text
        .bytes()
        .filter(|b| !matches!(b, b' ' | b'\t'))
        .collect();
    marks.len() >= 3
        && matches!(marks[0], b'*' | b'-' | b'_')
        && marks.iter().all(|b| *b == marks[0])
}

/// Byte width of a list item or block quote marker opening `text`, with the whitespace after it.
fn nested(text: &str) -> Option<usize> {
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    let marker = if text.starts_with(['>', '-', '*', '+']) {
        1
    } else if (1..=9).contains(&digits) && text[digits..].starts_with(['.', ')']) {
        digits + 1
    } else {
        return None;
    };
    let rest = &text[marker..];
    (text.starts_with('>') || rest.is_empty() || rest.starts_with([' ', '\t']))
        .then(|| text.len() - rest.trim_start().len())
}

/// Offset just past a leading YAML front matter block, which would otherwise read as a
/// setext heading; 0 when there is none or it is never closed.
fn front_matter(source: &str) -> usize {
    let mut lines = source.split_inclusive('\n');
    if lines.next().map(str::trim_end) != Some("---") {
        return 0;
    }
    let mut offset = source.find('\n').map_or(source.len(), |i| i + 1);
    for line in lines {
        offset += line.len();
        if matches!(line.trim_end(), "---" | "...") {
            return offset;
        }
    }
    0
}

struct Outline<'a> {
    path: &'a str,
    source: &'a str,
    evidence: Vec<Evidence>,
    /// Open sections, innermost last: level and index into `evidence`.
    open: Vec<(usize, usize)>,
}

impl Outline<'_> {
    fn bounded(&self, name: &str) -> Result<(), String> {
        if name.len() > 4096 {
            return Err("heading exceeds the name bound".into());
        }
        if self.evidence.len() >= 20_000 {
            return Err("extraction record bound exceeded".into());
        }
        Ok(())
    }

    /// A link or mention spanning `start..end`; its section is assigned once all are closed.
    fn point(
        &mut self,
        kind: &str,
        name: String,
        target: Option<String>,
        (start, end): (usize, usize),
        (line, column): (usize, usize),
    ) -> Result<(), String> {
        self.bounded(&name)?;
        self.evidence.push(Evidence {
            id: identity(&[self.path, PROVIDER, kind, &name, &start.to_string()]),
            kind: kind.into(),
            name,
            line,
            column,
            start_byte: start,
            end_byte: end,
            target,
            container: None,
        });
        Ok(())
    }

    /// Links and code-span mentions in one line of text starting at byte `at`, column `column`.
    fn inline(&mut self, text: &str, at: usize, line: usize, column: usize) -> Result<(), String> {
        if let Some((start, destination)) = definition(text) {
            return self.link(destination, at + start, line, column + start);
        }
        let bytes = text.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'\\' => i += 1 + text[i + 1..].chars().next().map_or(0, char::len_utf8),
                b'`' => {
                    let count = run(&text[i..], b'`');
                    let open = i + count;
                    let mut from = open;
                    let mut close = None;
                    while let Some(found) = text[from..].find('`') {
                        let end = from + found;
                        let length = run(&text[end..], b'`');
                        if length == count {
                            close = Some(end);
                            break;
                        }
                        from = end + length;
                    }
                    let Some(end) = close else {
                        i = open;
                        continue;
                    };
                    let inner = &text[open..end];
                    let lead = inner.len() - inner.trim_start().len();
                    let token = inner.trim();
                    if let Some(name) = mention(token) {
                        let start = open + lead;
                        let span = (at + start, at + start + token.len());
                        self.point(MENTION, name, None, span, (line, column + start))?;
                    }
                    i = end + count;
                }
                b']' if bytes.get(i + 1) == Some(&b'(') => {
                    let open = i + 2;
                    match text[open..].find(')') {
                        Some(length) => {
                            let inner = &text[open..open + length];
                            let lead = inner.len() - inner.trim_start().len();
                            self.link(
                                &inner[lead..],
                                at + open + lead,
                                line,
                                column + open + lead,
                            )?;
                            i = open + length + 1;
                        }
                        None => i += 1,
                    }
                }
                _ => i += 1,
            }
        }
        Ok(())
    }

    /// A local link destination spelled at `start`, recorded by its resolved path.
    fn link(
        &mut self,
        spelled: &str,
        start: usize,
        line: usize,
        column: usize,
    ) -> Result<(), String> {
        let (spelled, start, column) = match spelled.strip_prefix('<') {
            Some(inner) => (
                inner.split('>').next().unwrap_or_default(),
                start + 1,
                column + 1,
            ),
            None => (
                spelled.split([' ', '\t']).next().unwrap_or_default(),
                start,
                column,
            ),
        };
        let Some(path) = resolve(self.path, spelled) else {
            return Ok(());
        };
        let span = (start, start + spelled.len());
        self.point(LINK, path, Some(spelled.into()), span, (line, column))
    }

    /// Each link and mention's container: the innermost section enclosing it.
    fn assign_sections(&mut self) {
        let sections: Vec<(usize, usize, String)> = self
            .evidence
            .iter()
            .filter(|e| e.kind == KIND)
            .map(|e| (e.start_byte, e.end_byte, e.id.clone()))
            .collect();
        for item in self.evidence.iter_mut().filter(|e| e.kind != KIND) {
            // Sections nest in document order: the last one opening before the item that still
            // encloses it is the innermost.
            let before = sections.partition_point(|(start, ..)| *start <= item.start_byte);
            item.container = sections[..before]
                .iter()
                .rev()
                .find(|(start, end, _)| *start <= item.start_byte && item.end_byte <= *end)
                .map(|(.., id)| id.clone());
        }
    }

    /// Ends open sections of `level` or deeper at `at`, without trailing blank lines.
    fn close(&mut self, level: usize, at: usize) {
        while let Some(&(_, index)) = self.open.last().filter(|(open, _)| *open >= level) {
            let section = &mut self.evidence[index];
            let body = self.source[section.start_byte..at].trim_end();
            section.end_byte = section.start_byte + body.len();
            self.open.pop();
        }
    }

    /// Opens a section whose heading text starts at `start`; `line_start` begins its line.
    fn heading(
        &mut self,
        level: usize,
        name: String,
        (start, line, column): (usize, usize, usize),
        line_start: usize,
    ) -> Result<(), String> {
        self.bounded(&name)?;
        self.close(level, line_start);
        let container = self
            .open
            .last()
            .map(|&(_, index)| self.evidence[index].id.clone());
        self.evidence.push(Evidence {
            id: identity(&[self.path, PROVIDER, KIND, &name, &start.to_string()]),
            kind: KIND.into(),
            name,
            line,
            column,
            start_byte: start,
            end_byte: start + 1,
            target: None,
            container,
        });
        self.open.push((level, self.evidence.len() - 1));
        Ok(())
    }
}

pub fn extract(path: &str, bytes: &[u8]) -> Result<Extraction, String> {
    if !valid_path(path) || bytes.len() > MAX_SOURCE_BYTES {
        return Err("source exceeds path/byte bounds".into());
    }
    let source = std::str::from_utf8(bytes).map_err(|_| "source is not UTF-8")?;
    let skipped = front_matter(source);
    let mut outline = Outline {
        path,
        source,
        evidence: Vec::new(),
        open: Vec::new(),
    };
    let (mut paragraph, mut fenced, mut comment) = (Paragraph::None, None, false);
    let mut offset = 0;
    for (index, raw) in source.split_inclusive('\n').enumerate() {
        let line_start = offset;
        offset += raw.len();
        if line_start < skipped {
            continue;
        }
        let line = raw.trim_end_matches('\n').trim_end_matches('\r');
        let (columns, indent) = indentation(line);
        let text = &line[indent..];
        if let Some((marker, count)) = fenced {
            if columns < 4
                && run(text, marker) >= count
                && text[run(text, marker)..].trim().is_empty()
            {
                fenced = None;
            }
            continue;
        }
        if comment {
            comment = !line.contains("-->");
            continue;
        }
        if text.trim().is_empty() {
            paragraph = Paragraph::None;
            continue;
        }
        if columns >= 4 {
            // Indented code, unless it continues a paragraph.
            if let Paragraph::Plain(.., lines) = &mut paragraph {
                lines.push(text.trim().into());
                outline.inline(text, line_start + indent, index + 1, indent + 1)?;
            }
            continue;
        }
        let at = (line_start + indent, index + 1, indent + 1);
        let prose = fence(text).is_none()
            && !(matches!(paragraph, Paragraph::Plain(..)) && underline(text).is_some())
            && !thematic_break(text)
            && !text.starts_with("<!--");
        if prose {
            outline.inline(text, at.0, at.1, at.2)?;
        }
        if let Some(found) = fence(text) {
            fenced = Some(found);
            paragraph = Paragraph::None;
        } else if let Some((level, name)) = atx(text) {
            outline.heading(level, name, at, line_start)?;
            paragraph = Paragraph::None;
        } else if let (Paragraph::Plain(start, line, column, lines), Some(level)) =
            (&paragraph, underline(text))
        {
            let first = source[..*start].rfind('\n').map_or(0, |i| i + 1);
            let name = lines.join(" ");
            let at = (*start, *line, *column);
            outline.heading(level, name, at, first)?;
            paragraph = Paragraph::None;
        } else if thematic_break(text) {
            paragraph = Paragraph::None;
        } else if let Some(rest) = text.strip_prefix("<!--") {
            comment = !rest.contains("-->");
            paragraph = Paragraph::None;
        } else if let Some(mut inner) = nested(text) {
            // A list item or block quote may open with an ATX heading, also when nested.
            while let Some(more) = nested(&text[inner..]) {
                inner += more;
            }
            if let Some((level, name)) = atx(&text[inner..]) {
                let at = (at.0 + inner, at.1, at.2 + inner);
                outline.heading(level, name, at, line_start)?;
            }
            paragraph = Paragraph::Nested;
        } else if let Paragraph::Plain(.., lines) = &mut paragraph {
            lines.push(text.trim().into());
        } else if matches!(paragraph, Paragraph::None) {
            paragraph = Paragraph::Plain(at.0, at.1, at.2, vec![text.trim().into()]);
        }
    }
    outline.close(0, source.len());
    outline.assign_sections();
    if let Some(record) = decision(path, source, skipped, &outline.evidence) {
        outline.evidence.push(record);
    }
    let result = Extraction {
        path: path.into(),
        hash: hash(bytes),
        provider: PROVIDER.into(),
        evidence: outline.evidence,
        partial: false,
    };
    result.validate()?;
    Ok(result)
}

/// `[label]: destination` opening a line: the destination's offset and text.
fn definition(text: &str) -> Option<(usize, &str)> {
    let close = text.strip_prefix('[')?.find("]:")? + 1;
    if text[1..close].contains(['[', ']']) || close == 1 {
        return None;
    }
    let rest = &text[close + 2..];
    let destination = rest.trim_start();
    (!destination.is_empty()).then(|| (text.len() - destination.len(), destination))
}

/// A code span's content when it is one token naming something: no whitespace, at most 256
/// characters, a letter or digit in it; a call's trailing `()` is dropped.
fn mention(token: &str) -> Option<String> {
    let name = token.strip_suffix("()").unwrap_or(token);
    (!name.is_empty()
        && name.chars().count() <= 256
        && !name.contains(char::is_whitespace)
        && name.chars().any(char::is_alphanumeric))
    .then(|| name.to_string())
}

/// A link destination as a root-relative path: relative to the linking document, or to the
/// root with a leading `/`; the fragment and query are dropped. URLs, other schemes,
/// fragment-only links and paths leaving the root resolve to nothing.
pub fn resolve(document: &str, destination: &str) -> Option<String> {
    let path = destination.split(['#', '?']).next()?.replace("%20", " ");
    let scheme = path
        .split_once(':')
        .is_some_and(|(scheme, _)| !scheme.contains('/'));
    if path.is_empty() || scheme || path.starts_with("//") {
        return None;
    }
    let mut parts: Vec<&str> = match path.strip_prefix('/') {
        Some(_) => Vec::new(),
        None => document.split('/').collect(),
    };
    if !path.starts_with('/') {
        parts.pop();
    }
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    let resolved = parts.join("/");
    valid_path(&resolved).then_some(resolved)
}

/// Whether `path` names a decision record: a document under an ADR directory or named
/// `adr-*`, other than a readme, index or template.
fn decision_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let mut parts: Vec<&str> = lower.split('/').collect();
    let file = parts.pop().unwrap_or_default();
    let stem = file.split_once('.').map_or(file, |(stem, _)| stem);
    let named = stem.starts_with("adr-") || stem.starts_with("adr_");
    let placed = parts.iter().any(|part| DECISION_DIRECTORIES.contains(part));
    (named || placed) && !matches!(stem, "readme" | "index" | "template" | "_template")
}

/// A status line's text with its markup dropped and its links resolved, bounded.
fn status_text(document: &str, text: &str) -> Option<String> {
    let mut out = String::new();
    let mut rest = text.trim();
    while let Some(open) = rest.find('[') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find("](").and_then(|close| {
            let end = after[close + 2..].find(')')? + close + 2;
            Some((close, end))
        }) {
            Some((close, end)) => {
                let destination = &after[close + 2..end];
                out.push_str(
                    &resolve(document, destination).unwrap_or_else(|| after[..close].into()),
                );
                rest = &after[end + 1..];
            }
            None => {
                out.push('[');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    let status: String = out
        .replace(['*', '_', '`'], "")
        .trim()
        .chars()
        .take(STATUS_CHARS)
        .collect();
    (!status.is_empty()).then_some(status)
}

/// The status a decision record declares: front matter `status:`, a `Status:` line (also as a
/// list item or in bold) before the second heading, or the first line of a `Status` section.
fn status(document: &str, source: &str, skipped: usize) -> Option<String> {
    let field = |line: &str| -> Option<String> {
        let line = line.trim().trim_start_matches(['-', '*', '+', '>', ' ']);
        let plain = line.replace("**", "").replace("__", "");
        let (key, value) = plain.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case("status")
            .then(|| status_text(document, value))
            .flatten()
    };
    if let Some(found) = source[..skipped].lines().find_map(field) {
        return Some(found);
    }
    let (mut headings, mut section) = (0, false);
    for line in source[skipped..].lines() {
        let trimmed = line.trim();
        if let Some((_, name)) = atx(trimmed) {
            headings += 1;
            section = name.eq_ignore_ascii_case("status");
            continue;
        }
        if section && !trimmed.is_empty() {
            return status_text(document, trimmed.trim_start_matches(['-', '*', '+', ' ']));
        }
        if headings <= 1
            && let Some(found) = field(line)
        {
            return Some(found);
        }
    }
    None
}

/// The decision record a document at `path` is, from its first section to its end.
fn decision(path: &str, source: &str, skipped: usize, evidence: &[Evidence]) -> Option<Evidence> {
    if !decision_path(path) {
        return None;
    }
    let title = evidence.iter().find(|e| e.kind == KIND)?;
    let end = source.trim_end().len().max(title.start_byte + 1);
    Some(Evidence {
        id: identity(&[
            path,
            PROVIDER,
            DECISION,
            &title.name,
            &title.start_byte.to_string(),
        ]),
        kind: DECISION.into(),
        name: title.name.clone(),
        line: title.line,
        column: title.column,
        start_byte: title.start_byte,
        end_byte: end,
        target: status(path, source, skipped),
        container: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sections(source: &str) -> Vec<(String, usize, String, Option<String>)> {
        let result = extract("doc.md", source.as_bytes()).unwrap();
        let name = |id: &Option<String>| {
            id.as_ref().map(|id| {
                result
                    .evidence
                    .iter()
                    .find(|e| &e.id == id)
                    .unwrap()
                    .name
                    .clone()
            })
        };
        result
            .evidence
            .iter()
            .filter(|e| e.kind == KIND)
            .map(|e| {
                (
                    e.name.clone(),
                    e.line,
                    source[e.start_byte..e.end_byte].to_string(),
                    name(&e.container),
                )
            })
            .collect()
    }

    #[test]
    fn sections_run_to_the_next_heading_of_the_same_or_higher_level() {
        let source = "# Guide\nintro\n\n## Install ##\nsteps\n\n### Linux\nApt\n\n## Use\nrun it\n";
        assert_eq!(
            sections(source),
            vec![
                ("Guide".into(), 1, source.trim_end().into(), None),
                (
                    "Install".into(),
                    4,
                    "## Install ##\nsteps\n\n### Linux\nApt".into(),
                    Some("Guide".into())
                ),
                (
                    "Linux".into(),
                    7,
                    "### Linux\nApt".into(),
                    Some("Install".into())
                ),
                (
                    "Use".into(),
                    10,
                    "## Use\nrun it".into(),
                    Some("Guide".into())
                ),
            ]
        );
    }

    #[test]
    fn code_comments_and_front_matter_are_not_headings_but_list_item_headings_are() {
        let source = "---\ntitle: x\n---\nSetext\ntitle\n======\n```sh\n# shell comment\n```\n~~~~\n## still code\n~~~\n~~~~~\n<!--\n# hidden\n-->\n    # indented code\n- item\n---\n* ## Listed\n> - # Quoted\n#hashtag\n  ## Indented #\nText\n---\n";
        let found: Vec<_> = sections(source)
            .into_iter()
            .map(|(name, line, ..)| (name, line))
            .collect();
        assert_eq!(
            found,
            vec![
                ("Setext title".into(), 4),
                ("Listed".into(), 20),
                ("Quoted".into(), 21),
                ("Indented".into(), 23),
                ("Text".into(), 24)
            ]
        );
    }

    #[test]
    fn crlf_offsets_cite_the_heading_and_bounds_refuse() {
        let result = extract("a.md", b"intro\r\n\r\n  # Title\r\nbody\r\n").unwrap();
        let section = &result.evidence[0];
        assert_eq!(
            (section.line, section.column, section.start_byte),
            (3, 3, 11)
        );
        assert_eq!(
            &b"intro\r\n\r\n  # Title\r\nbody\r\n"[section.start_byte..section.end_byte],
            b"# Title\r\nbody"
        );
        assert!(extract("a.md", &[255]).is_err());
        assert!(extract("a.md", format!("# {}", "x".repeat(4097)).as_bytes()).is_err());
        assert!(extract("a.md", b"no headings").unwrap().evidence.is_empty());
    }

    fn points(path: &str, source: &str) -> Vec<(String, String, usize, Option<String>, String)> {
        let result = extract(path, source.as_bytes()).unwrap();
        let section = |id: &Option<String>| {
            id.as_ref()
                .map(|id| {
                    result
                        .evidence
                        .iter()
                        .find(|e| &e.id == id)
                        .unwrap()
                        .name
                        .clone()
                })
                .unwrap_or_default()
        };
        result
            .evidence
            .iter()
            .filter(|e| e.kind == LINK || e.kind == MENTION)
            .map(|e| {
                (
                    e.kind.clone(),
                    e.name.clone(),
                    e.line,
                    e.target.clone(),
                    section(&e.container),
                )
            })
            .collect()
    }

    #[test]
    fn links_resolve_against_the_document_and_code_spans_are_mentions() {
        let source = "# Guide\nSee [the engine](../engine/src/lib.rs#L5 \"title\") and `craft_engine::Evidence`.\n\n## Use\nRun `locate()` or ``a ` b``; [web](https://x.y), [top](#guide), [up](../../out.md).\n\n```\n`not` [code](a.md)\n```\n    `indented`\n[ref]: /docs/ADR.md\n- item ![img](<img dir/p.png>) \\`escaped`\n";
        assert_eq!(
            points("docs/guide.md", source),
            vec![
                (
                    "md:link".into(),
                    "engine/src/lib.rs".into(),
                    2,
                    Some("../engine/src/lib.rs#L5".into()),
                    "Guide".into()
                ),
                (
                    "md:mention".into(),
                    "craft_engine::Evidence".into(),
                    2,
                    None,
                    "Guide".into()
                ),
                ("md:mention".into(), "locate".into(), 5, None, "Use".into()),
                (
                    "md:link".into(),
                    "docs/ADR.md".into(),
                    11,
                    Some("/docs/ADR.md".into()),
                    "Use".into()
                ),
                (
                    "md:link".into(),
                    "docs/img dir/p.png".into(),
                    12,
                    Some("img dir/p.png".into()),
                    "Use".into()
                ),
            ]
        );
        assert_eq!(resolve("a/b.md", "./c/../d.md"), Some("a/d.md".into()));
        assert_eq!(resolve("a/b.md", "mailto:x@y"), None);
        assert_eq!(resolve("a.md", "src/"), Some("src".into()));
        // A mention on a setext heading's own text belongs to that heading.
        let setext = points("a.md", "# A\n\n`x` title\n-----\nbody `y`\n");
        assert_eq!(
            setext.iter().map(|p| p.4.as_str()).collect::<Vec<_>>(),
            ["`x` title", "`x` title"]
        );
    }

    #[test]
    fn decision_records_carry_title_and_status_with_resolved_links() {
        let decision = |path: &str, source: &str| {
            extract(path, source.as_bytes())
                .unwrap()
                .evidence
                .into_iter()
                .find(|e| e.kind == DECISION)
                .map(|e| (e.name, e.line, e.target))
        };
        let madr = "# Use the ADR number\n\n- Status: superseded by [20201016-slug](20201016-slug.md)\n- Date: 2020-09-26\n\n## Context\nStatus: not this\n";
        assert_eq!(
            decision("docs/adr/20200926-number.md", madr),
            Some((
                "Use the ADR number".into(),
                1,
                Some("superseded by docs/adr/20201016-slug.md".into())
            ))
        );
        assert_eq!(
            decision(
                "docs/adr/0001-rust.md",
                "# ADR 0001: Rust\n\nStatus: Accepted — 2026-10-22\n"
            ),
            Some((
                "ADR 0001: Rust".into(),
                1,
                Some("Accepted — 2026-10-22".into())
            ))
        );
        assert_eq!(
            decision("design/ADR-7.md", "---\nstatus: proposed\n---\n# Seven\n"),
            Some(("Seven".into(), 4, Some("proposed".into())))
        );
        assert_eq!(
            decision("decisions/x.md", "# X\n\n## Status\n\n**Accepted**\n"),
            Some(("X".into(), 1, Some("Accepted".into())))
        );
        assert_eq!(decision("decisions/x.md", "# X\nbody\n").unwrap().2, None);
        assert_eq!(decision("docs/adr/template.md", "# T\nStatus: x\n"), None);
        assert_eq!(decision("docs/guide.md", "# G\nStatus: x\n"), None);
        assert_eq!(decision("docs/adr/empty.md", "no heading"), None);
    }
}
