//! Configuration keys as cited facts: every key of a JSON, TOML or YAML mapping, named by its
//! dotted path from the document root (`compilerOptions.strict`, `jobs.build.runs-on`), with
//! its scalar value as the target and the enclosing key as its container. Scanners, not full
//! parsers: keys inside arrays (JSON arrays, TOML arrays of tables, YAML sequences) are not
//! recorded, inline tables and flow collections are values, not descended, and a YAML plain
//! scalar continued on a line that reads `word: text` is taken for a key. A segment that
//! itself contains `.` is joined as spelled, so a dotted name can be ambiguous. JSON may carry
//! `//` and `/* */` comments and trailing commas; structurally broken JSON is refused.
//! A lockfile (`package-lock.json`, `pnpm-lock.yaml`) or a file over `CONFIGURATION_BYTES` is
//! data rather than configuration: its keys are not extracted and it is recorded partial, so a
//! key lookup searches it literally instead of reading an empty answer as absence.
use crate::syntax::MAX_SOURCE_BYTES;
use craft_engine::{Evidence, Extraction, hash, identity, valid_path};
use std::collections::HashMap;

pub const PROVIDER: &str = "config-keys/v1";
pub const KIND: &str = "config:key";
const RECORDS: usize = 20_000;
const NAME_BYTES: usize = 4096;
const TARGET_CHARS: usize = 256;
const DEPTH: usize = 128;
/// Largest file whose keys are extracted: project configuration measured at most 24 KiB on the
/// probe repositories, test data and lockfiles from 200 KiB up.
pub const CONFIGURATION_BYTES: usize = 64 * 1024;

fn lockfile(path: &str) -> bool {
    let file = path.rsplit('/').next().unwrap_or(path);
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    stem.ends_with("-lock") || stem.ends_with(".lock") || stem == "lock"
}

struct Key {
    name: String,
    start: usize,
    end: usize,
    target: Option<String>,
    /// Index of the enclosing recorded key; always lower than this key's own index.
    parent: Option<usize>,
}

#[derive(Default)]
struct Keys(Vec<Key>);

/// A scalar value as cited: trimmed, at most `TARGET_CHARS`, nothing when empty.
fn bounded(text: &str) -> Option<String> {
    let text = text.trim();
    let text = match text.char_indices().nth(TARGET_CHARS) {
        Some((end, _)) => &text[..end],
        None => text,
    };
    (!text.is_empty()).then(|| text.to_string())
}

impl Keys {
    fn add(
        &mut self,
        name: String,
        (start, end): (usize, usize),
        target: Option<&str>,
        parent: Option<usize>,
    ) -> Result<usize, String> {
        if name.len() > NAME_BYTES {
            return Err("key exceeds the name bound".into());
        }
        if self.0.len() >= RECORDS {
            return Err("extraction record bound exceeded".into());
        }
        self.0.push(Key {
            name,
            start,
            end: end.max(start + 1),
            target: target.and_then(bounded),
            parent,
        });
        Ok(self.0.len() - 1)
    }
}

fn join(parent: Option<&str>, key: &str) -> String {
    parent.map_or_else(|| key.to_string(), |parent| format!("{parent}.{key}"))
}

/// Byte offset of `inner`, a subslice of `outer`, within it.
fn offset(outer: &str, inner: &str) -> usize {
    inner.as_ptr() as usize - outer.as_ptr() as usize
}

/// `text` before a `#` comment that starts it or follows whitespace, outside quotes.
fn uncommented(text: &str) -> &str {
    let bytes = text.as_bytes();
    let (mut at, mut quote) = (0, None);
    while at < bytes.len() {
        let token = at == 0
            || matches!(
                bytes[at - 1],
                b' ' | b'\t' | b'=' | b':' | b',' | b'[' | b'{'
            );
        match (quote, bytes[at]) {
            (Some(b'"'), b'\\') => at += 1,
            (Some(open), byte) if byte == open => quote = None,
            (Some(_), _) => {}
            (None, byte @ (b'"' | b'\'')) if token => quote = Some(byte),
            (None, b'#') if at == 0 || matches!(bytes[at - 1], b' ' | b'\t') => {
                return &text[..at];
            }
            _ => {}
        }
        at += 1;
    }
    text
}

/// Open brackets and braces after `text`, starting from `depth`; quotes and comments skipped.
fn brackets(text: &str, mut depth: usize) -> usize {
    let bytes = uncommented(text).as_bytes();
    let (mut at, mut quote) = (0, None);
    while at < bytes.len() {
        match (quote, bytes[at]) {
            (Some(b'"'), b'\\') => at += 1,
            (Some(open), byte) if byte == open => quote = None,
            (Some(_), _) => {}
            (None, byte @ (b'"' | b'\'')) => quote = Some(byte),
            (None, b'[' | b'{') => depth += 1,
            (None, b']' | b'}') => depth = depth.saturating_sub(1),
            _ => {}
        }
        at += 1;
    }
    depth
}

fn unquote(text: &str) -> &str {
    ['"', '\'']
        .into_iter()
        .find(|q| text.len() >= 2 && text.starts_with(*q) && text.ends_with(*q))
        .map_or(text, |_| &text[1..text.len() - 1])
}

pub fn extract(path: &str, bytes: &[u8]) -> Result<Extraction, String> {
    if !valid_path(path) || bytes.len() > MAX_SOURCE_BYTES {
        return Err("source exceeds path/byte bounds".into());
    }
    let source = std::str::from_utf8(bytes).map_err(|_| "source is not UTF-8")?;
    if bytes.len() > CONFIGURATION_BYTES || lockfile(path) {
        let result = Extraction {
            path: path.into(),
            hash: hash(bytes),
            provider: PROVIDER.into(),
            evidence: Vec::new(),
            partial: true,
        };
        result.validate()?;
        return Ok(result);
    }
    let extension = path
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default();
    let mut keys = Keys::default();
    match extension.as_str() {
        "json" | "jsonc" => Json {
            source,
            at: 0,
            keys: &mut keys,
        }
        .document()?,
        "toml" => toml(source, &mut keys)?,
        "yaml" | "yml" => yaml(source, &mut keys)?,
        _ => return Err("unsupported configuration extension".into()),
    }
    let mut keys = keys.0;
    // A key's range covers every key recorded under it; children follow their parent.
    for index in (0..keys.len()).rev() {
        if let Some(parent) = keys[index].parent {
            keys[parent].end = keys[parent].end.max(keys[index].end);
        }
    }
    let starts: Vec<usize> = std::iter::once(0)
        .chain(source.match_indices('\n').map(|(at, _)| at + 1))
        .collect();
    let mut evidence: Vec<Evidence> = keys
        .iter()
        .map(|key| {
            let line = starts.partition_point(|start| *start <= key.start);
            Evidence {
                id: identity(&[path, PROVIDER, KIND, &key.name, &key.start.to_string()]),
                kind: KIND.into(),
                name: key.name.clone(),
                line,
                column: key.start - starts[line - 1] + 1,
                start_byte: key.start,
                end_byte: key.end,
                target: key.target.clone(),
                container: None,
            }
        })
        .collect();
    for (index, key) in keys.iter().enumerate() {
        evidence[index].container = key.parent.map(|parent| evidence[parent].id.clone());
    }
    let result = Extraction {
        path: path.into(),
        hash: hash(bytes),
        provider: PROVIDER.into(),
        evidence,
        partial: false,
    };
    result.validate()?;
    Ok(result)
}

/// Where an object's keys go: `None` inside an array (not recorded), `Some(None)` at the
/// root, `Some(Some((name, index)))` under a recorded key.
type Scope<'s> = Option<Option<(&'s str, usize)>>;

struct Json<'a> {
    source: &'a str,
    at: usize,
    keys: &'a mut Keys,
}

impl<'a> Json<'a> {
    fn bytes(&self) -> &'a [u8] {
        self.source.as_bytes()
    }

    fn document(&mut self) -> Result<(), String> {
        self.value(0, Some(None))?;
        self.space()?;
        match self.at == self.source.len() {
            true => Ok(()),
            false => Err("content after the JSON value".into()),
        }
    }

    fn space(&mut self) -> Result<(), String> {
        loop {
            match self.bytes().get(self.at..) {
                Some([b' ' | b'\t' | b'\r' | b'\n', ..]) => self.at += 1,
                Some([b'/', b'/', ..]) => {
                    self.at = self.source[self.at..]
                        .find('\n')
                        .map_or(self.source.len(), |at| self.at + at);
                }
                Some([b'/', b'*', ..]) => {
                    let close = self.source[self.at + 2..]
                        .find("*/")
                        .ok_or("unterminated JSON comment")?;
                    self.at += 2 + close + 2;
                }
                _ => return Ok(()),
            }
        }
    }

    /// The content range of the string opening at `at`; `at` moves past its closing quote.
    fn string(&mut self) -> Result<(usize, usize), String> {
        let start = self.at + 1;
        let mut at = start;
        loop {
            match self.bytes().get(at) {
                None | Some(b'\n') => return Err("unterminated JSON string".into()),
                Some(b'\\') => at += 2,
                Some(b'"') => {
                    self.at = at + 1;
                    return Ok((start, at));
                }
                _ => at += 1,
            }
        }
    }

    fn value(&mut self, depth: usize, scope: Scope<'_>) -> Result<(), String> {
        if depth > DEPTH {
            return Err("JSON nesting exceeds the bound".into());
        }
        self.space()?;
        let key = scope.flatten().map(|(_, index)| index);
        let mut target = None;
        match self.bytes().get(self.at) {
            Some(b'{') => self.object(depth, scope)?,
            Some(b'[') => self.array(depth)?,
            Some(b'"') => {
                let (start, end) = self.string()?;
                target = bounded(&self.source[start..end]);
            }
            Some(_) => {
                let start = self.at;
                while self.bytes().get(self.at).is_some_and(|byte| {
                    !matches!(
                        byte,
                        b',' | b']'
                            | b'}'
                            | b'{'
                            | b'['
                            | b'"'
                            | b':'
                            | b'/'
                            | b' '
                            | b'\t'
                            | b'\r'
                            | b'\n'
                    )
                }) {
                    self.at += 1;
                }
                if self.at == start {
                    return Err("JSON value expected".into());
                }
                target = bounded(&self.source[start..self.at]);
            }
            None => return Err("JSON value expected".into()),
        }
        if let Some(index) = key {
            let key = &mut self.keys.0[index];
            key.end = key.end.max(self.at);
            key.target = target;
        }
        Ok(())
    }

    /// Separator after a member or element: `,` (a trailing one is allowed) or `close`.
    fn next(&mut self, close: u8) -> Result<bool, String> {
        self.space()?;
        match self.bytes().get(self.at) {
            Some(b',') => {
                self.at += 1;
                self.space()?;
            }
            Some(byte) if *byte == close => {}
            _ => return Err("JSON ',' or closing bracket expected".into()),
        }
        let closed = self.bytes().get(self.at) == Some(&close);
        if closed {
            self.at += 1;
        }
        Ok(closed)
    }

    fn object(&mut self, depth: usize, scope: Scope<'_>) -> Result<(), String> {
        self.at += 1;
        self.space()?;
        if self.bytes().get(self.at) == Some(&b'}') {
            self.at += 1;
            return Ok(());
        }
        loop {
            if self.bytes().get(self.at) != Some(&b'"') {
                return Err("JSON object key expected".into());
            }
            let (start, end) = self.string()?;
            self.space()?;
            if self.bytes().get(self.at) != Some(&b':') {
                return Err("JSON ':' expected".into());
            }
            self.at += 1;
            let key = match scope {
                Some(parent) => {
                    let name = join(parent.map(|(name, _)| name), &self.source[start..end]);
                    let index =
                        self.keys
                            .add(name.clone(), (start, end), None, parent.map(|(_, i)| i))?;
                    Some((name, index))
                }
                None => None,
            };
            let scope = key
                .as_ref()
                .map(|(name, index)| Some((name.as_str(), *index)));
            self.value(depth + 1, scope)?;
            if self.next(b'}')? {
                return Ok(());
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<(), String> {
        self.at += 1;
        self.space()?;
        if self.bytes().get(self.at) == Some(&b']') {
            self.at += 1;
            return Ok(());
        }
        loop {
            self.value(depth + 1, None)?;
            if self.next(b']')? {
                return Ok(());
            }
        }
    }
}

/// A TOML dotted key at the start of `text` (absolute offset `base`): each segment as spelled
/// with its content range, and the text after it.
/// A key segment as spelled and its byte range.
type Segment = (String, usize, usize);

fn key_path(text: &str, base: usize) -> Option<(Vec<Segment>, &str)> {
    let bytes = text.as_bytes();
    let blank = |at: &mut usize| {
        while matches!(bytes.get(*at), Some(b' ' | b'\t')) {
            *at += 1;
        }
    };
    let (mut segments, mut at) = (Vec::new(), 0);
    loop {
        blank(&mut at);
        let (start, end, next) = match *bytes.get(at)? {
            quote @ (b'"' | b'\'') => {
                let mut end = at + 1;
                while *bytes.get(end)? != quote {
                    end += if quote == b'"' && bytes[end] == b'\\' {
                        2
                    } else {
                        1
                    };
                }
                (at + 1, end, end + 1)
            }
            _ => {
                let length = bytes[at..]
                    .iter()
                    .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
                    .count();
                if length == 0 {
                    return None;
                }
                (at, at + length, at + length)
            }
        };
        segments.push((text[start..end].to_string(), base + start, base + end));
        at = next;
        blank(&mut at);
        if bytes.get(at) != Some(&b'.') {
            return Some((segments, &text[at..]));
        }
        at += 1;
    }
}

/// The nearest recorded key enclosing `name`.
fn ancestor(known: &HashMap<String, usize>, mut name: &str) -> Option<usize> {
    while let Some((parent, _)) = name.rsplit_once('.') {
        if let Some(index) = known.get(parent) {
            return Some(*index);
        }
        name = parent;
    }
    None
}

fn toml(source: &str, keys: &mut Keys) -> Result<(), String> {
    let mut known = HashMap::new();
    let mut arrays: Vec<String> = Vec::new();
    let (mut table, mut recording) = (None::<String>, true);
    // An open multi-line string's delimiter, and brackets left open by a multi-line value.
    let (mut string, mut depth) = (None::<&str>, 0);
    let mut offset_at = 0;
    for raw in source.split_inclusive('\n') {
        let start = offset_at;
        offset_at += raw.len();
        let line = raw.trim_end_matches(['\n', '\r']);
        if let Some(delimiter) = string {
            if line.contains(delimiter) {
                string = None;
            }
            continue;
        }
        if depth > 0 {
            depth = brackets(line, depth);
            continue;
        }
        let text = line.trim();
        if text.is_empty() || text.starts_with('#') {
            continue;
        }
        let at = start + offset(line, text);
        if text.starts_with('[') {
            let array = text.starts_with("[[");
            let open = 1 + usize::from(array);
            let Some((segments, rest)) = key_path(&text[open..], at + open) else {
                continue;
            };
            if !rest.starts_with(if array { "]]" } else { "]" }) {
                continue;
            }
            let name: Vec<&str> = segments.iter().map(|s| s.0.as_str()).collect();
            let name = name.join(".");
            let under_array = arrays.iter().any(|a| {
                name.strip_prefix(a.as_str())
                    .is_some_and(|r| r.starts_with('.'))
            });
            let span = (segments[0].1, segments[segments.len() - 1].2);
            recording = !array && !under_array;
            table = Some(name.clone());
            if (recording || (array && !under_array)) && !arrays.contains(&name) {
                let index = keys.add(name.clone(), span, None, ancestor(&known, &name))?;
                known.insert(name.clone(), index);
            }
            if array && !arrays.contains(&name) {
                arrays.push(name);
            }
            continue;
        }
        let Some((segments, rest)) = key_path(text, at) else {
            continue;
        };
        let Some(value) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        let value = value.trim_start();
        let value_at = start + offset(line, value);
        let key_end = segments[segments.len() - 1].2;
        let (target, end) = if let Some(delimiter) = ["\"\"\"", "'''"]
            .into_iter()
            .find(|d| value.starts_with(*d))
        {
            match value[3..].find(delimiter) {
                Some(close) => (Some(&value[3..3 + close]), value_at + close + 6),
                None => {
                    string = Some(delimiter);
                    (None, key_end)
                }
            }
        } else if value.starts_with(['[', '{']) {
            depth = brackets(value, 0);
            let shown = uncommented(value).trim_end();
            (
                None,
                if depth == 0 {
                    value_at + shown.len()
                } else {
                    key_end
                },
            )
        } else {
            let scalar = uncommented(value).trim_end();
            (Some(unquote(scalar)), value_at + scalar.len())
        };
        if recording {
            let spelled: Vec<&str> = segments.iter().map(|s| s.0.as_str()).collect();
            let name = join(table.as_deref(), &spelled.join("."));
            let parent = ancestor(&known, &name);
            let index = keys.add(name.clone(), (segments[0].1, end), target, parent)?;
            known.insert(name, index);
        }
    }
    Ok(())
}

/// A YAML mapping key opening `text`: its content range and the text after its `:`.
fn yaml_key(text: &str) -> Option<(usize, usize, &str)> {
    let bytes = text.as_bytes();
    let separated = |at: usize| matches!(bytes.get(at), None | Some(b' ' | b'\t'));
    match bytes[0] {
        quote @ (b'"' | b'\'') => {
            let close = text[1..].find(quote as char)? + 1;
            let tail = text[close + 1..].trim_start_matches([' ', '\t']);
            let colon = close + 1 + (text.len() - close - 1 - tail.len());
            (tail.starts_with(':') && separated(colon + 1)).then(|| (1, close, &text[colon + 1..]))
        }
        b'[' | b'{' | b'?' | b'!' | b'&' | b'*' | b'|' | b'>' | b'@' | b'`' | b'%' | b'#' => None,
        _ => {
            let colon = text
                .match_indices(':')
                .map(|(at, _)| at)
                .find(|at| separated(at + 1))?;
            let key = text[..colon].trim_end();
            (!key.is_empty() && !key.contains(" #")).then(|| (0, key.len(), &text[colon + 1..]))
        }
    }
}

fn yaml(source: &str, keys: &mut Keys) -> Result<(), String> {
    // Open mappings, innermost last: indentation, dotted name, key index.
    let mut open: Vec<(usize, String, usize)> = Vec::new();
    // Lines indented deeper than this belong to a block scalar or a sequence item.
    let (mut skip, mut depth) = (None::<usize>, 0);
    let mut offset_at = 0;
    for raw in source.split_inclusive('\n') {
        let start = offset_at;
        offset_at += raw.len();
        let line = raw.trim_end_matches(['\n', '\r']);
        let text = line.trim_start_matches(' ');
        let indent = line.len() - text.len();
        if depth > 0 {
            depth = brackets(line, depth);
            continue;
        }
        if text.trim().is_empty() || text.starts_with('#') {
            continue;
        }
        if skip.is_some_and(|floor| indent > floor) {
            continue;
        }
        skip = None;
        let marker =
            text.starts_with("---") && (text.len() == 3 || text[3..].starts_with([' ', '\t']));
        if indent == 0 && (marker || text == "..." || text.starts_with('%')) {
            open.clear();
            continue;
        }
        while open.last().is_some_and(|(at, ..)| *at >= indent) {
            open.pop();
        }
        if text == "-" || text.starts_with("- ") || text.starts_with("-\t") {
            skip = Some(indent);
            continue;
        }
        let Some((key_start, key_end, rest)) = yaml_key(text) else {
            continue;
        };
        let parent = open.last().map(|(_, name, index)| (name.as_str(), *index));
        let name = join(parent.map(|(name, _)| name), &text[key_start..key_end]);
        let parent = parent.map(|(_, index)| index);
        let span = (start + indent + key_start, start + indent + key_end);
        let value = uncommented(rest).trim();
        let value_at = start + offset(line, value);
        if value
            .split_whitespace()
            .all(|word| word.starts_with(['&', '!']))
        {
            let index = keys.add(name.clone(), span, None, parent)?;
            open.push((indent, name, index));
        } else if value.starts_with(['|', '>']) {
            keys.add(name, span, None, parent)?;
            skip = Some(indent);
        } else if value.starts_with(['[', '{']) {
            depth = brackets(value, 0);
            let end = if depth == 0 {
                value_at + value.len()
            } else {
                span.1
            };
            keys.add(name, (span.0, end), None, parent)?;
        } else {
            keys.add(
                name,
                (span.0, value_at + value.len()),
                Some(unquote(value)),
                parent,
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    type Row = (String, usize, Option<String>, Option<String>);

    fn keys(path: &str, source: &str) -> Vec<Row> {
        let result = extract(path, source.as_bytes()).unwrap();
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
            .map(|e| (e.name.clone(), e.line, e.target.clone(), name(&e.container)))
            .collect()
    }

    fn row(name: &str, line: usize, target: Option<&str>, container: Option<&str>) -> Row {
        (
            name.into(),
            line,
            target.map(Into::into),
            container.map(Into::into),
        )
    }

    #[test]
    fn json_paths_nest_with_containers_and_skip_keys_inside_arrays() {
        let source = "{\n  \"name\": \"pkg\",\n  \"compilerOptions\": {\n    \"strict\": true,\n    \"paths\": {\"@/*\": [\"src/*\"]}\n  },\n  \"files\": [{\"skip\": 1}]\n}\n";
        assert_eq!(
            keys("tsconfig.json", source),
            vec![
                row("name", 2, Some("pkg"), None),
                row("compilerOptions", 3, None, None),
                row(
                    "compilerOptions.strict",
                    4,
                    Some("true"),
                    Some("compilerOptions")
                ),
                row("compilerOptions.paths", 5, None, Some("compilerOptions")),
                row(
                    "compilerOptions.paths.@/*",
                    5,
                    None,
                    Some("compilerOptions.paths")
                ),
                row("files", 7, None, None),
            ]
        );
        let commented = "// tsconfig\n{\n  /* options */\n  \"compilerOptions\": { \"target\": \"es2022\", // modern\n    \"lib\": [\"dom\",], },\n}\n";
        assert_eq!(
            keys("tsconfig.jsonc", commented),
            vec![
                row("compilerOptions", 4, None, None),
                row(
                    "compilerOptions.target",
                    4,
                    Some("es2022"),
                    Some("compilerOptions")
                ),
                row("compilerOptions.lib", 5, None, Some("compilerOptions")),
            ]
        );
        assert!(keys("a.json", "[1, {\"a\": 2}]").is_empty());
        for broken in [
            "{\"a\" 1}",
            "{\"a\": 1",
            "{\"a\": \"x}",
            "{\"a\": 1} 2",
            "{,}",
            "/* open",
        ] {
            assert!(extract("a.json", broken.as_bytes()).is_err(), "{broken}");
        }
    }

    #[test]
    fn toml_tables_dotted_keys_and_multi_line_values() {
        let source = "title = \"x\" # comment\n[tool.poetry]\nname = 'demo'\ndescription = \"\"\"\nkey = \"not a key\"\n\"\"\"\n[tool.poetry.dependencies]\nserde = { version = \"1\" }\n\"quoted.key\" = 1\nlist = [\n  \"a = b\",\n]\nsite.url = \"https://x\"\n[[bin]]\nname = \"skipped\"\n[after]\nok = true\n";
        let deps = Some("tool.poetry.dependencies");
        assert_eq!(
            keys("pyproject.toml", source),
            vec![
                row("title", 1, Some("x"), None),
                row("tool.poetry", 2, None, None),
                row("tool.poetry.name", 3, Some("demo"), Some("tool.poetry")),
                row("tool.poetry.description", 4, None, Some("tool.poetry")),
                row("tool.poetry.dependencies", 7, None, Some("tool.poetry")),
                row("tool.poetry.dependencies.serde", 8, None, deps),
                row("tool.poetry.dependencies.quoted.key", 9, Some("1"), deps),
                row("tool.poetry.dependencies.list", 10, None, deps),
                row(
                    "tool.poetry.dependencies.site.url",
                    13,
                    Some("https://x"),
                    deps
                ),
                row("bin", 14, None, None),
                row("after", 16, None, None),
                row("after.ok", 17, Some("true"), Some("after")),
            ]
        );
    }

    #[test]
    fn yaml_nesting_skips_sequences_block_scalars_and_comments() {
        let source = "# workflow\nname: CI\non:\n  push:\n    branches: [main]\njobs:\n  build:\n    runs-on: ubuntu-latest # comment\n    steps:\n      - uses: actions/checkout@v4\n        with:\n          depth: 1\n      - run: |\n          echo key: value\n    \"quoted key\": 'v'\n    script: >\n      folded: text\n    env: &defaults\n      A: 1\n---\nsecond: doc\n";
        let build = Some("jobs.build");
        assert_eq!(
            keys(".github/workflows/ci.yml", source),
            vec![
                row("name", 2, Some("CI"), None),
                row("on", 3, None, None),
                row("on.push", 4, None, Some("on")),
                row("on.push.branches", 5, None, Some("on.push")),
                row("jobs", 6, None, None),
                row("jobs.build", 7, None, Some("jobs")),
                row("jobs.build.runs-on", 8, Some("ubuntu-latest"), build),
                row("jobs.build.steps", 9, None, build),
                row("jobs.build.quoted key", 15, Some("v"), build),
                row("jobs.build.script", 16, None, build),
                row("jobs.build.env", 18, None, build),
                row("jobs.build.env.A", 19, Some("1"), Some("jobs.build.env")),
                row("second", 21, Some("doc"), None),
            ]
        );
    }

    #[test]
    fn crlf_offsets_cite_the_key_and_bounds_refuse() {
        let source = b"{\r\n  \"a\": {\r\n    \"b\": 1\r\n  }\r\n}\r\n";
        let result = extract("a.json", source).unwrap();
        let b = &result.evidence[1];
        assert_eq!((b.line, b.column, b.start_byte), (3, 6, 18));
        assert_eq!(&source[b.start_byte..b.end_byte], b"b\": 1");
        assert_eq!(
            keys("a.yaml", "a:\r\n  b: x\r\n"),
            vec![row("a", 1, None, None), row("a.b", 2, Some("x"), Some("a"))]
        );
        assert!(extract("a.json", &[255]).is_err());
        assert!(extract("a.ini", b"a=1").is_err());
        let long = format!("{{\"{}\": 1}}", "k".repeat(4097));
        assert!(extract("a.json", long.as_bytes()).is_err());
    }

    #[test]
    fn data_sized_files_are_partial_without_keys() {
        let large = format!("{{\"a\": \"{}\"}}", "x".repeat(CONFIGURATION_BYTES));
        let result = extract("fixture.json", large.as_bytes()).unwrap();
        assert!(result.partial && result.evidence.is_empty());
        for lock in ["package-lock.json", "web/pnpm-lock.yaml"] {
            assert!(extract(lock, b"{\"a\": 1}").unwrap().partial, "{lock}");
        }
        assert!(!extract("blocking.json", b"{\"a\": 1}").unwrap().partial);
    }
}
