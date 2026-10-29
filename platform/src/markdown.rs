//! Markdown sections as cited passages: a heading's range runs to the next heading of the
//! same or a higher level, and its container is the enclosing section. A line scanner, not a
//! full CommonMark parser: inside list items and block quotes only an ATX heading opening the
//! item or quote line is recognized, HTML blocks other than comments are not, and inline
//! markup stays as spelled in the name.
use crate::syntax::MAX_SOURCE_BYTES;
use craft_engine::{Evidence, Extraction, hash, identity, valid_path};

pub const PROVIDER: &str = "markdown-sections/v1";
const KIND: &str = "md:section";

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
        if name.len() > 4096 {
            return Err("heading exceeds the name bound".into());
        }
        if self.evidence.len() >= 20_000 {
            return Err("extraction record bound exceeded".into());
        }
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
            }
            continue;
        }
        let at = (line_start + indent, index + 1, indent + 1);
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
    let result = Extraction {
        path: path.into(),
        hash: hash(bytes),
        provider: PROVIDER.into(),
        evidence: outline.evidence,
    };
    result.validate()?;
    Ok(result)
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
}
